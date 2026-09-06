# es-toolkit final 45 — group: ARRAY ELEMENTS / HOLES / UNDEFINED RESULTS

Read-only investigation (no cargo). Generated crate read at
`third_party/es-toolkit/dist-smelt/src/`, verified against the prebuilt probe
`target/debug/deps/es_toolkit_probe-60af05449d054ba8`.

Five distinct roots cover the nine assigned tests. Two of them (truthiness collapse,
tuple-overload over-match) are high blast radius and silently corrupt library functions
whose own specs do not currently catch them.

---

## Root A — JavaScript truthiness of an erased / type-parameter value is not lowered as truthiness

Two sibling functions in the TS frontend answer a truthiness guard with something that is
not a truthiness test:

`crates/smelt-frontend-ts/src/lowering/new_expr.rs`, `lowered_condition_expression`
(statement/expression conditions), lines ~2998-3008:

```rust
if matches!(
    self.ctx.krate.types.get(cond_ty),
    Some(Type::Function(_) | Type::Class { .. } | Type::TypeParam { .. })
) {
    let ty = self.ctx.krate.types.intern(Type::Bool);
    return Ok(body.push_expr(Expr {
        kind: ExprKind::Literal(Literal::Bool(true)),   // <-- constant true for `T`
        ty,
        span,
    }));
}
```

`crates/smelt-frontend-ts/src/lowering/callbacks/body_lowering.rs`,
`callback_truthy_expression` (conditions inside a lowered callback), lines ~540-547 and
~611-619:

```rust
if matches!(
    self.ctx.krate.types.get(expr_ty),
    Some(Type::Function(_) | Type::Class { .. } | Type::TypeParam { .. })
) {
    return Ok(CallbackExpr { kind: CallbackExprKind::Literal(Literal::Bool(true)), .. });
}
...
if self.ctx.krate.types.get(expr_ty) == Some(&Type::Unknown) {
    return Ok(CallbackExpr {
        kind: CallbackExprKind::UnknownIs { value: Box::new(expr), kind: UnknownKind::Bool },
        ..
    });   // <-- `typeof x === "boolean"`, not `!!x`
}
```

`Type::Class`/`Type::Function` really are always truthy in JS, so those arms are fine. But
an unconstrained `T` and an erased `unknown` can hold `0`, `-0`, `NaN`, `''`, `false`,
`null`, `undefined`. `UnknownIs { kind: Bool }` renders through
`crates/smelt-codegen-rust/src/emitter/coercion.rs::tag_check_raw` as
`matches!(x, SmeltUnknown::Bool(_))` — it answers *true for `false`* and *false for a
truthy string/number*, i.e. it is close to the inverse of the intended test.

The correct lowering already exists and is used elsewhere: `PrimitiveCast { op: ToBool }`,
which `crates/smelt-codegen-rust/src/emitter/types.rs::primitive_cast_text`
(arm `(ToBool, Type::Bool, Type::Unknown | Type::Union(_) | Type::TypeParam { .. } | Type::Never)`)
emits as the full JS truthiness match. `CallbackExprKind::FieldTruthy` (used for
`x?.field` guards) already routes through it in
`crates/smelt-frontend-ts/src/lowering/callbacks/closures.rs::callback_expr_to_body_expr`.

### A1 — `compact_spec::test_compact_removes_falsey_values_from_array`

* Spec: `third_party/es-toolkit/src/array/compact.spec.ts:6`
  `expect(compact([0, -0, 0n, 1, false, 2, '', 3, null, undefined, 4, NaN, 5])).toEqual([1, 2, 3, 4, 5])`
* Source: `src/array/compact.ts:19` — `if (item) { result.push(item as NotFalsey<T>); }`, `item: T`.
* Generated `dist-smelt/src/compact_1.rs:21-24`:

```rust
item = arr.borrow().get({ let normalized = i as i64; usize::try_from(normalized).unwrap_or(usize::MAX) }).cloned().unwrap_or_else(|| SmeltUnknown::Undefined);
if true {
_smelt_tmp_7 = item;
_smelt_tmp_8 = { let smelt_push_item = _smelt_tmp_7; result.borrow_mut().push(smelt_push_item); result.len() as f64 };
```

`if (item)` became `if true`, so `compact` copies the array verbatim: 13 elements instead
of 5. (Note the `0n` BigInt literal is emitted as `SmeltUnknown::Number(0.0)`, which is
falsy and therefore *not* a contributing defect here — the whole guard is gone.)

* Layer: frontend-ts, `lowered_condition_expression` (`Type::TypeParam` arm).

### A2 — `dropWhile_spec::…returns_false_from_the_beginning`

* Spec: `src/array/dropWhile.spec.ts:14` — `expect(dropWhile(items, x => !x.enabled)).toEqual([{id:2,enabled:true},{id:3,enabled:false}])`
* Generated `dist-smelt/src/dropWhile_spec.rs:43-47`:

```rust
_smelt_tmp_10 = ::std::rc::Rc::new(|closure_arg_0: SmeltRecord<String, SmeltUnknown>| {
let _smelt_tmp_1: bool = matches!(closure_arg_0.get(&"enabled".to_owned()).unwrap_or(SmeltUnknown::Undefined).clone(), SmeltUnknown::Bool(_));
let _smelt_tmp_2: bool = !(_smelt_tmp_1);
_smelt_tmp_2
});
```

`x.enabled` is `SmeltUnknown`; `!x.enabled` became `!(typeof x.enabled === "boolean")`.
For `{id:1,enabled:false}` the tag check is `true`, so the predicate answers `false` at
index 0 → `findIndex` returns `0` → `arr.slice(0)` → the whole array is returned.

* Layer: frontend-ts, `callback_truthy_expression` (`Type::Unknown` arm).

### A3 — `dropRightWhile_spec::…returns_false_from_the_end`

Identical generated code, `dist-smelt/src/dropRightWhile_spec.rs:44` (same
`matches!(… , SmeltUnknown::Bool(_))` line). The last element `{id:3,enabled:false}` makes
`canContinueDropping` answer `false` at `i = 2`, so `arr.slice(0, 3)` returns all three
elements instead of the first two. Same root as A2.

### Blast radius beyond this group

Grepping the generated crate for both smells:

* `if true {` — `compact_1.rs:22`, `invertBy.rs:79`, `some.rs:227,269,311`
  (`src/compat/array/some.ts:114` `if (predicate(source[i] as T, i, source))` → `if true { return Ok(true); }`, i.e. `some` unconditionally returns `true` for the array path).
* `matches!(…, SmeltUnknown::Bool(_))` as a truthiness test — `find.rs`, `findIndex.rs` (10 sites), `findLast.rs`, `findLastIndex.rs` (10), `findLastKey.rs`, `takeWhile.rs`, `takeRightWhile.rs`, `isKey.rs`, `negate.rs:13`, `random.rs`.
  `negate.rs:13` is the same shape as the failing
  `__smelt_module_negate_spec::test_negate_should_negate_the_given_predicate_function`
  in `failures.txt` (another group) — likely the same root.

### Fix design (Root A)

1. Add `CallbackExprKind::ValueTruthy { value: Box<CallbackExpr> }` to
   `crates/smelt-hir/src/expr/call.rs`, lowered in
   `callbacks/closures.rs::callback_expr_to_body_expr` to
   `ExprKind::PrimitiveCast { op: ToBool, operand }` — exactly the existing `FieldTruthy`
   arm, minus the field read. Thread it through the `CallbackExprKind` walkers already
   listed for `FieldTruthy` (`closures.rs:183`, `body_lowering.rs:352,1608`,
   `classify.rs:392`).
2. In `callback_truthy_expression`, replace the `Type::Unknown` →
   `UnknownIs { kind: Bool }` return with `ValueTruthy`, and narrow the
   `Function | Class | TypeParam` → `Literal(true)` arm to
   `Function | Class` plus `TypeParam` **whose constraint satisfies
   `type_is_always_truthy_object_surface`**; every other `TypeParam` returns `ValueTruthy`.
3. In `lowered_condition_expression`, narrow the same
   `Function | Class | TypeParam` → `Literal(true)` arm the same way. Unconstrained
   `TypeParam` then falls through to the existing
   `type_is_truthy_condition_surface` → `PrimitiveCast(ToBool)` path, which the emitter
   already handles for `Type::TypeParam`.
* Regression test shape: two frontend/codegen snapshot tests —
  `fn f<T>(x: T) { if (x) … }` and `[…].filter(x => !x.field)` where `field: unknown` —
  asserting the emitted Rust contains the `SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan()`
  truthiness match and contains neither `if true` nor `SmeltUnknown::Bool(_)`; plus a
  runtime test that a generic `compact`-shaped function drops `0`, `''`, `false`, `NaN`.
* Size: **S–M** (two arms + one HIR variant; the walker plumbing is mechanical).

---

## Root B — a fixed-arity / non-empty tuple *parameter* accepts any list argument, so overload selection picks the wrong signature

`crates/smelt-frontend-ts/src/lowering/stdlib/call_dispatch.rs::infer_overload_type`,
lines ~3782-3786:

```rust
(Some(Type::Tuple(expected_items)), Some(Type::List(actual_item))) => {
    expected_items.into_iter().all(|expected_item| {
        self.infer_overload_type(expected_item, actual_item, substitutions)
    })
}
```

A `Type::Tuple` parameter of *any* arity matches a `Type::List` argument of *unknown*
length. Combined with `selected_overload_signature`'s tiebreak
(`(literal_score, specificity, none_collapse, usize::MAX - order)` — equal specificity, so
earliest declaration wins) the first-declared tuple overload swallows plain array calls
that TypeScript would route to a later overload.

The second half of the same root: `readonly [T, ...T[]]` (TS's non-empty array) is lowered
to plain `Type::List(T)` by
`crates/smelt-frontend-ts/src/lowering/ty/annotations.rs::homogeneous_tuple_rest_type`
("HIR does not currently track non-empty length refinements"), so a non-empty-array
parameter is indistinguishable from `T[]` and again the earlier declaration wins. The
analogous problem for *rest* parameters was already solved with `rest_parameter_min_arity`
+ `OverloadSignature::min_rest` (used in `overload_signature_matches_args`); the fixed
parameter position has no equivalent.

### B1 — `initial_spec::test_initial_returns_all_elements_except_the_last_one_for_a_large_array`

* Spec: `src/array/initial.spec.ts:24` — `expect(initial(largeArray)).toEqual(expectedArray)`, `largeArray: number[]` of 1000 elements.
* Overloads: `src/array/initial.ts:13` `initial<T>(arr: readonly [T]): []` is declared FIRST; the applicable one is line 55 `initial<T>(arr: readonly T[]): T[]`.
* `readonly [T]` → `Type::Tuple([T])`, which matches `SmeltList<i64>` via the arm above, and its return type `[]` → the empty tuple → Rust `()`.
* Generated `dist-smelt/src/initial_spec.rs:99-100`:

```rust
let _smelt_tmp_12: () = { let smelt_tuple_values = initial_111(large_array).to_vec(); () };
_smelt_tmp_13 = !(SmeltUnknown::Array(vec![].into()) == { let smelt_l = expected_array; … });
```

The real result of `initial_111` (which is correctly typed `SmeltList<T> -> SmeltList<T>`,
`dist-smelt/src/initial_1.rs:7`) is computed and thrown away; the comparison uses a literal
empty array against the 999-element expectation.

**Worse than the failure:** the three sibling assertions in the same spec pass *vacuously*.
`dist-smelt/src/initial_spec.rs:23-25`:

```rust
let _smelt_tmp_1: () = { let smelt_tuple_values = initial_111(_smelt_tmp_0).to_vec(); () };
_smelt_tmp_2 = ();
_smelt_tmp_3 = _smelt_tmp_1 != _smelt_tmp_2;      // () != ()  ==  false
```

so `expect(initial([1,2,3])).toEqual([1,2])` is compiled to `false` and reported as
passing. Confirmed by the probe: 3 pass, 1 fails.

### B2 / B3 — `maxBy_spec::test_maxby_if_array_is_empty_return_undefined`, `minBy_spec::test_minby_if_array_is_empty_return_undefined`

* Spec: `src/array/maxBy.spec.ts:31-36` / `src/array/minBy.spec.ts:31-36` —
  `const people: Person[] = []; const result = maxBy(people, p => p.age); expect(result).toBeUndefined();`
* Overloads: `src/array/maxBy.ts:22` declares `maxBy<T>(items: readonly [T, ...T[]], getValue): T` FIRST; the applicable one is line 48 `maxBy<T>(items: readonly T[], getValue): T | undefined`.
* `readonly [T, ...T[]]` collapses to `Type::List(T)` (`homogeneous_tuple_rest_type`), so
  both overloads match with identical specificity and order wins → the call site's return
  type is `T`, not `T | undefined`.
* Generated `dist-smelt/src/maxBy_spec.rs:87,94,96` (minBy identical at
  `minBy_spec.rs:87,94,96`):

```rust
let result: Person;
let _smelt_tmp_4: Person = max_by_120(…).clone().map_or(Default::default(), |value| … );
…
_smelt_tmp_5 = !(false);
```

The implementation is emitted correctly — `max_by_120` returns `Option<T>`
(`dist-smelt/src/maxBy_1.rs:7`) and does return `None` for an empty list. The damage is at
the call site: the `Optional -> concrete` coercion
(`crates/smelt-codegen-rust/src/emitter/coercion.rs`, the
`if let Some(Type::Optional(inner)) = … { … map_or({default}, |value| …) }` arm at ~344)
unwraps the `None` into `Person::default()`, and because `result: Person` is not optional,
`expect(result).toBeUndefined()` is folded to the constant `false`.

### Fix design (Root B)

Give overload parameters a static length requirement and demand call-site evidence for it,
mirroring the existing `min_rest` machinery:

1. Extend `OverloadSignature` with `param_min_len: Vec<Option<usize>>` (or a
   `HashMap<usize, ParamLen>` carrying `Exact(n)` / `AtLeast(n)`), filled in
   `module_init.rs::overload_signature` from the *source* annotation:
   a rest-less tuple `[A, B]` → `Exact(2)`; a required-prefix tuple `[T, ...T[]]` and
   `NonEmptyArray<T>` → `AtLeast(1)` (reuse `rest_parameter_min_arity`, which already
   computes exactly this from a `TSType`).
2. In `overload_signature_matches_args`, before `infer_overload_type` on a fixed
   parameter with a length requirement, require the *source argument* to prove it: an
   `ArrayExpression` with no spread and the right element count (`>= n` for `AtLeast`,
   `== n` for `Exact`). A variable of type `T[]`, a call result, or a spread proves
   nothing and must fail the match. The function already receives `arguments: &[Argument]`
   and already inspects source shape this way
   (`argument_is_empty_array_literal`, `overload_source_arg_matches_param`), so this is a
   local addition.
3. With (2) in place, the permissive `(Tuple, List)` arm in `infer_overload_type` can stay
   for the cases where the length is proven (it is reached only after the gate), so no
   existing tuple-hint behaviour regresses.
4. Independently worth hardening: an assertion whose two sides both lower to Rust `()`
   should be a lowering error, not a silently passing `() != ()`. Cheap guard in
   `lowering/testing/matchers.rs`; it would have surfaced B1 three assertions earlier.
* Regression test shape: an overload trio
  `f<T>(a: readonly [T]): []; f<T>(a: readonly [T, ...T[]]): T; f<T>(a: readonly T[]): T | undefined;`
  and three call sites — `f([1])` (tuple overload), `f(varOfTypeNumberArray)` (list
  overload, must return `Option`), `f([])` (list overload) — asserting the selected return
  type in the emitted signature; plus a runtime test that the empty-array call answers
  `undefined`.
* Size: **M**.

---

## Root C — a list→list coercion whose element map is the identity rebuilds the buffer and breaks aliasing

### C1 — `remove_spec::test_remove_should_handle_sparse_arrays_correctly`

* Spec: `src/array/remove.spec.ts:16` — `expect(sparseArray).toEqual([1, 3, 5])` after
  `remove(sparseArray, value => value === undefined)`. `remove` mutates `arr` in place
  (`src/array/remove.ts:36,39`).
* `remove_130` is emitted correctly with a by-reference parameter
  (`dist-smelt/src/remove_1.rs:7`):
  `fn remove_130(mut arr: &mut SmeltList<SmeltUnknown>, …) -> SmeltList<SmeltUnknown>`.
* Generated `dist-smelt/src/remove_spec.rs:55` (argument position):

```rust
remove_130(&mut { let smelt_l: SmeltList<_> = sparse_array.clone().into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| value).collect::<Vec<_>>()) }, … )
```

`sparse_array` is already `SmeltList<SmeltUnknown>` and the element map is
`|value| value` — a pure identity. But `SmeltList::with_id(id, Vec)` allocates a **new**
`Rc<RefCell<Vec<_>>>` (`dist-smelt/src/main.rs:2390`), unlike the aliasing
`with_storage` (`main.rs:2392`). So `remove_130` mutates a temporary and `sparse_array`
is unchanged: it still reads `[1, undefined, 3, undefined, 5]` at line 58.

* Layer: codegen-rust emitter,
  `crates/smelt-codegen-rust/src/emitter/coercion.rs::value_at_type`, the
  `(Type::List(source_item), Type::List(target_item))` arm at lines ~355-391. Its guard is
  `source_item != target_item` — a **TypeId** comparison. Two distinct TypeIds that render
  to the same Rust type (here `Type::Unknown` vs `Type::TypeParam { .. }`, both
  `SmeltUnknown`) pass the guard, so the arm fires and rebuilds the buffer for nothing.
  Precedent for the intended shape is already in the erased-array path
  (`crates/smelt-codegen-rust/src/lib.rs:2149-2152` "Reuse an existing shared buffer, so a
  re-wrap keeps aliasing the same array", with the comment at 2202 noting the old
  copying form `with_id(list.id(), list.into_vec())` was the bug).
* Sparse-hole modelling is *not* implicated: `[1, , 3, , 5]` lowers to explicit
  `SmeltUnknown::Undefined` elements (`remove_spec.rs:49`), which yields the same
  `[1,3,5]` / `[undefined, undefined]` answers as JS for this predicate.

### Fix design (Root C)

In the `(List, List)` arm of `value_at_type`, compute
`let element_text = self.value_at_type_text("value", *source_item, *target_item)?` first
and, when it is the identity (`"value"`, i.e. the two element types share one Rust
rendering), emit an aliasing re-wrap instead of a rebuild:

```rust
{ let smelt_l: SmeltList<_> = {op}; SmeltList::with_storage(smelt_l.id(), smelt_l.storage()) }
```

or, better, return the operand text unchanged (no `.clone()` of the buffer at all). The
cleanest formulation of the general rule: **compare Rust renderings, not TypeIds** — add a
`same_rust_repr(a, b)` helper and use it as the arm's guard, so the whole arm is skipped
when the element representation is unchanged. Then teach the argument-position emitter to
refuse a *value-rebuilding* coercion for a by-reference parameter (fail the emit rather
than silently drop the mutation) so a genuinely representation-changing case cannot
regress the same way.
* Regression test shape: codegen snapshot for `f(arr)` where `f(arr: T[])` mutates
  `arr` in place and the call site's list has a different element TypeId but the same
  rendering — assert the emitted argument contains `with_storage` (or is the bare local)
  and never `with_id(`; plus a runtime test that the caller's array observes the mutation.
* Size: **S**.

---

## Root D — an out-of-range element read yields the element type's `Default`, not `undefined`

### D1 — `at_spec::test_at_should_return_undefined_for_non_integer_indices`

**Spec-file confirmation (the prior report doubted this):** the generated module
`__smelt_module_at_spec` comes from **`src/array/at.spec.ts`**, not
`src/compat/object/at.spec.ts`. `dist-smelt/src/at_spec.rs:2` reads
`// source: third_party/es-toolkit/src/array/at.spec.ts`, its five test names are that
file's five `it(…)` blocks, and it calls `at_0` from
`dist-smelt/src/at_1.rs` (`// source: …/src/array/at.ts`). The compat function is
`at_570` in `dist-smelt/src/at.rs` and is not referenced by this spec.

* Spec: `src/array/at.spec.ts:23-28`

```ts
const data = ['a', 'b', 'c'];
const indices = [1.5, -1.5, NaN, Infinity, -Infinity];
expect(at(data, indices)).toEqual(indices.map(i => data.at(i)));
```

  Both sides answer `['b', 'c', 'a', undefined, undefined]` in JS.
* The right-hand side is emitted correctly (`dist-smelt/src/at_spec.rs:128-129`): the
  `data.at(i)` read produces `Option<String>` and then
  `map_or(SmeltUnknown::Undefined, |value| SmeltUnknown::String(...))` → the two misses
  become `Undefined`. Good.
* The left-hand side is wrong. `at_0` is instantiated at `T = String` (because
  `const data = ['a','b','c']` is typed `SmeltList<String>`), and every element read
  substitutes `Default::default()`. `dist-smelt/src/at_1.rs:41` (and the two identical
  copies at :58 and :72):

```rust
let smelt_assign_value = arr.borrow().get({ let normalized = index as i64; usize::try_from(normalized).unwrap_or(usize::MAX) }).cloned().unwrap_or_else(|| Default::default());
```

  For `index = Infinity` (→ `i64::MAX`) and `-Infinity` (→ negative after `+ length`) the
  `get` misses and the slot receives `String::new()`. The comparison at `at_spec.rs:141`
  therefore pits `SmeltUnknown::String("")` against `SmeltUnknown::Undefined`.
  (`Number.isInteger` → `index.fract() == 0.0` and the `as i64` saturating casts are all
  correct here: `1.5→1`, `-1.5→-1`, `NaN→0`, `Infinity→i64::MAX`.)
  `at_1.rs:20`'s `new Array<T>(indices.length)` → `(0..n).map(|_| Default::default())`
  has the same shape, though every slot is overwritten in this test.
* Layer: codegen-rust emitter,
  `crates/smelt-codegen-rust/src/emitter/place.rs::element_missing_value_text` (lines
  811-828). It answers `SmeltUnknown::Undefined` only when the item type is
  `Unknown | TypeParam | Union` with no concrete union members; for a concrete item type
  it falls through to `self.default_value(item_ty)`. Consumed by the `Place::Index`
  read arm at `place.rs:339-386` (`… .get(idx).cloned().unwrap_or_else(|| {missing})`).
* This is exactly why the sibling test at `at.spec.ts:16`
  (`expect(at(['a','b','c'],[2,4,0,-4])).toEqual(['c', undefined, 'a', undefined])`)
  **passes**: there the argument is an inline literal typed `SmeltList<SmeltUnknown>`
  (`at_spec.rs:82`), so `T = SmeltUnknown`, whose `Default` *is* `Undefined`
  (`crates/smelt-codegen-rust/src/lib.rs:3359-3363`). The behaviour is decided by an
  accident of how the argument was spelled.

### Fix design (Root D)

The honest general rule is TypeScript's `noUncheckedIndexedAccess`: a list element read is
`T | undefined` unless the index is provably in range, and a hand-writing Rust team would
give `at` the signature `fn at<T: Clone>(arr: &[T], indices: &[f64]) -> Vec<Option<T>>`.
Concretely:

1. In HIR/MIR, type a `Place::Index` read of `Type::List(T)` as `Type::Optional(T)` when
   the index is not provably in range, and let the existing flow narrowing
   (`if (x)`, `!= null`, `?? d`) recover `T` — most reads are immediately consumed at `T`
   and would insert a `unwrap_or_default()` at the consumer, which is where the JS
   semantics already agree.
2. Propagate that to the *storing* side: when a possibly-absent read is stored into a list
   or returned, the destination element type widens to `Optional<T>`, so
   `at`'s `result: Array<T>` becomes `SmeltList<Option<String>>` and erases to
   `SmeltUnknown::Undefined` at the boundary. `element_missing_value_text` then loses its
   `default_value` fallback entirely (a `None` is always representable), which is the
   property to assert.
3. A strictly smaller interim step that removes the spelling accident without the full
   analysis: when instantiating a generic function whose body can produce an absent
   element that reaches its return value, instantiate `T` at `Optional<concrete>` rather
   than at the bare concrete type. This is narrower but still a general rule; it does not
   fix an out-of-range read that stays inside one monomorphic function.
* Regression test shape: `const a = ['x']; a[5]` must not emit `String::new()`; a runtime
  test that `at(['a','b','c'], [Infinity])` and `['a'][5]` both answer `undefined` when
  the source list is typed `string[]` **and** when it is typed `unknown[]` — i.e. the two
  spellings must agree.
* Size: **L** for (1)+(2); **M** for (3).

---

## Root E — a nullish value is coerced into a non-nullable concrete slot instead of widening the slot

### E1 — `zip_spec::test_zip_zips_multiple_arrays_to_create_a_tuple`

* Spec: `src/array/zip.spec.ts:13-17` —
  `expect(zip([1, 2, 3], ['a', 'b'])).toEqual([[1,'a'],[2,'b'],[3, undefined]])`.
* `zip`'s declared overload gives the actual the type `[number, string][]`, so both sides
  are shaped `SmeltList<(f64, String)>` — and each side loses the `undefined` **in a
  different direction**, which is what makes the assertion fail rather than pass
  vacuously:
  * Expected side, `dist-smelt/src/zip_spec.rs:78`:
    `_smelt_tmp_23 = (3.0, String::new());`
    The `undefined` element received the tuple element hint `String` and was emitted as
    `Default`. `array_element_hint_matches_arity`
    (`crates/smelt-frontend-ts/src/lowering/expr/operators.rs:2818-2848`, whose own
    docstring names "zip's trailing `[3, undefined]`") keeps the hint because the arity
    matches (2 == 2), and nothing checks that the *element* is nullish while the hint is
    not nullable.
  * Actual side, `dist-smelt/src/zip_spec.rs:75`: the erased element coming out of
    `zip_160` is projected into the `String` slot with the JS **`String(x)` coercion**:
    `… SmeltUnknown::Undefined => "undefined".to_owned(), …`.
    So slot 1 holds `"undefined"` where the expectation holds `""`.
* Layers: frontend-ts (`array_expression` / `array_element_with_hint` in
  `lowering/expr/operators.rs`) for the expected side; codegen-rust
  (`emitter/coercion.rs::extract` / `value_at_type_text`, the erased→`String` arm) for the
  actual side.

### Fix design (Root E)

1. Frontend: in `array_expression`, drop or widen a non-nullable element hint for a
   nullish element — if the element lowers to `Literal::None`/`Literal::Undefined` and the
   hint is not `Optional`/`Unknown`/a union containing `None`, use
   `Optional(hint)` for that slot and widen the literal's tuple/list type accordingly.
   This is the same shape as the existing arity guard next to it, and it is what
   `array_literal_item_type` already does for the un-hinted case (lines 2745-2763).
2. Emitter: extracting an erased value into a concrete non-nullable type must not fall
   back to the JS `String()`/`Number()` coercion for `SmeltUnknown::Undefined`/`Null`.
   Either the target is `Optional<T>` (→ `None`) or the extraction is a defect to reject at
   emit time. The `"undefined"`-string and `String::new()` fallbacks disagreeing with each
   other is the concrete evidence that they are papering over a missing `Optional`.
* Regression test shape: `expect([1, undefined]).toEqual([1, undefined])` where the
  contextual hint is `(f64, String)` — assert both sides emit `None`/`Undefined` for slot 1
  and never `String::new()` or `"undefined".to_owned()`; plus a runtime test on
  `zip([1,2,3], ['a','b'])`.
* Size: **M**.

---

## Correction to a prior report

`blocker-logs/estk-remaining-triage.md:33` files `compact`, `remove`, `dropWhile` and
`dropRightWhile` under "array element reads … an out-of-range read substitutes
`Default::default()` instead of `undefined`". That is wrong for all four: `compact`,
`dropWhile` and `dropRightWhile` are Root A (truthiness lowering) and `remove` is Root C
(list aliasing). Only `at` in this group is the `Default`-for-missing family, and `maxBy` /
`minBy` reach a `Default::default()` only as a *consequence* of Root B mis-typing the call
site. The line's `zip` entry is Root E, and its `maxBy`/`minBy` entries should move to
Root B.

---

## Summary

| test | root family | verdict | size |
| --- | --- | --- | --- |
| `compact_spec::test_compact_removes_falsey_values_from_array` | **A** truthiness of `T` collapsed to `if true` (`lowered_condition_expression`) | (a) general defect, fixable | S–M |
| `dropWhile_spec::…returns_false_from_the_beginning` | **A** truthiness of `unknown` lowered as `matches!(x, SmeltUnknown::Bool(_))` (`callback_truthy_expression`) | (a) general defect, fixable | S–M |
| `dropRightWhile_spec::…returns_false_from_the_end` | **A** same as dropWhile | (a) general defect, fixable | S–M (same fix) |
| `initial_spec::…for_a_large_array` | **B** `Type::Tuple` param matches any `Type::List` arg → first overload wins, return `[]` → `()` | (a) general defect, fixable | M |
| `maxBy_spec::test_maxby_if_array_is_empty_return_undefined` | **B** `readonly [T, ...T[]]` erased to `List(T)` → first overload wins, return `T` not `T \| undefined` | (a) general defect, fixable | M (same fix) |
| `minBy_spec::test_minby_if_array_is_empty_return_undefined` | **B** same as maxBy | (a) general defect, fixable | M (same fix) |
| `remove_spec::test_remove_should_handle_sparse_arrays_correctly` | **C** identity list re-wrap uses `with_id` (new buffer) instead of `with_storage`, so an in-place mutation is lost | (a) general defect, fixable | S |
| `at_spec::test_at_should_return_undefined_for_non_integer_indices` | **D** out-of-range element read → `Default::default()` (`element_missing_value_text`) | (a) general defect, fixable | L (M for the interim rule) |
| `zip_spec::test_zip_zips_multiple_arrays_to_create_a_tuple` | **E** nullish coerced into a non-nullable concrete slot, and the two coercions disagree (`""` vs `"undefined"`) | (a) general defect, fixable | M |

No test in this group is out of scope. Roots A and B also make currently-*passing*
assertions meaningless (`some` always returns `true`; three `initial` assertions compile to
`() != ()`), so fixing them will likely expose further failures rather than only close
these.
