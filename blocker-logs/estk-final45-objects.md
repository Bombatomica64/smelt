# es-toolkit final 45 — OBJECTS / MERGE / PROTOTYPE / CLONE group

Read-only investigation (no cargo). Every claim below was re-verified against the
**current** generated crate at `third_party/es-toolkit/dist-smelt/src/` and the
prebuilt probe binary; earlier blocker-log diagnoses were not trusted.

Seven distinct roots cover the eight tests. Two of them (`A`, `B`) also own
failures outside this group.

---

## 1. `invert` — should not invert inherited properties

**Spec** `third_party/es-toolkit/src/object/invert.spec.ts:41-45`

```ts
const object = Object.create({ a: 1 });
object.b = 2;
expect(invert(object)).toEqual({ 2: 'b' });   // JS: only the OWN key 'b'
```

**Generated Rust** — `dist-smelt/src/invert_spec.rs:153-157`

```rust
let _smelt_tmp_1: SmeltRecord<String, SmeltUnknown> =
    SmeltRecord::from([("__smelt_proto:a".to_owned(), SmeltUnknown::Number(1.0 as f64))]);
...
invert_730(if let SmeltUnknown::Object(values) = object.clone() {
    SmeltJsMap::from_iter(values.into_iter()
        .map(|(key, value)| (SmeltUnknown::String(key.into()), (value).into_smelt_unknown())))
} else { SmeltJsMap::new() })
```

`Object.create({a:1})` is correctly encoded as the inherited-member key
`"__smelt_proto:a"`, but the erased-object → `SmeltJsMap` coercion copies that
marker key in **verbatim as an ordinary string key**. `invert`'s key loop then
enumerates it (`dist-smelt/src/invert_1.rs:16`):

```rust
let _smelt_tmp_7: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into(
    obj.clone().keys().filter(|key| !matches!(key, SmeltUnknown::Symbol(_))).collect::<Vec<_>>());
```

so `keys == ["__smelt_proto:a", "b"]` and the result is
`{ "1": "__smelt_proto:a", "2": "b" }` instead of `{ "2": "b" }`.

**Root layer: codegen-rust emitter.** Two seams, both wrong for the same reason:

* `crates/smelt-codegen-rust/src/emitter/coercion.rs:2366-2378`
  (`extract_value_text`, `Type::Dict` with a non-`String` key →
  `SmeltJsMap::from_iter(...)`) — no marker filter.
* `crates/smelt-codegen-rust/src/emitter/map.rs:823-826`
  (`DictProjectionOp::Keys`, `map_op_uses_js_key_map` branch) — symbol-only
  filter. Its own comment at `map.rs:805-809` asserts the premise that is false:
  *"the `SmeltJsMap` and plain dict backings keep the symbol-only filter (they
  never carry internal markers …)"*. They do carry them, precisely because of
  the coercion above.

**Fix design (general).** Own-key enumeration must honour the marker convention
on *every* backing, not only `SmeltRecord`. Preferred: add
`smelt_own_js_map_keys(&SmeltJsMap<SmeltUnknown, V>) -> Vec<SmeltUnknown>` to the
prelude (`crates/smelt-codegen-rust/src/lib.rs`, next to
`smelt_for_in_object_keys` at `lib.rs:1959`), which drops
`__smelt_proto:` / `__smelt_method:` / `__smelt_class` keys and maps
`__smelt_symbol:x` back to `SmeltUnknown::Symbol(x)`; use it from the `Keys` /
`Values` / `Entries` / `ForInKeys` JsMap branches of `emitter/map.rs`. Filtering
at the coercion instead is the smaller diff but loses the inherited-property
*lookup* fallback (`obj[k]` for an inherited `k`), so it is the worse of the two.

**Shared root:** none of my other tests; this is the row the earlier triage
listed as *"`Object.keys` on a `SmeltJsMap` does not filter inherited keys"*
(`blocker-logs/estk-remaining-triage.md:196`) — that description is right, but the
*cause* is the coercion, not the map.

**Regression test shape:** codegen runtime test — `Object.create({a:1})`, add an
own key, pass into a `function f(o: Record<PropertyKey, unknown>)` that returns
`Object.keys(o)`; assert one key.

**Verdict: (a) general defect. Size S.**

---

## 2. `merge` — should behave like recursive Object.assign

**Spec** `third_party/es-toolkit/src/object/merge.spec.ts:122-131`

```ts
const topLevelArray = merge(['1'], { a: 2 });
expect(Array.isArray(topLevelArray)).toBe(true);   // JS: arr is still an array, plus a named prop
```

**Generated Rust** — `merge.rs` assigns through the erased index-store helper:

```rust
{ let smelt_key = key.clone(); let smelt_value = source_value; smelt_index_assign(&mut target, smelt_key, smelt_value); }
```

and the helper (`crates/smelt-codegen-rust/src/lib.rs:3043-3060`) is:

```rust
SmeltUnknown::Array(array) => {
    if let Ok(index) = key.parse::<usize>() { array.set_index(index, value); }
    else { *target = SmeltUnknown::Object(SmeltObject::new(Vec::from([(key, value)]))); }
}
```

Assigning the non-index key `"a"` **replaces the whole array with a fresh
single-property object** — the elements are lost and `Array.isArray` (emitted as
`matches!(top_level_array, SmeltUnknown::Array(_))`, `merge_spec.rs:464`) is
`false`.

The static-field twin is even blunter — `emitter/control_flow.rs:535-539`:

```rust
match &mut array1 { SmeltUnknown::Object(map) => { map.insert("every".to_owned(), smelt_value); },
                    other => { *other = SmeltUnknown::Object(SmeltObject::new(...)); } }
```

(seen verbatim at `isEqualWith_spec.rs:1168`), with no array arm at all.

**Root layer: runtime representation + emitter.** `SmeltList`
(`crates/smelt-runtime/src/value/list.rs:55-58`) is `{ id, values: Rc<RefCell<Vec<T>>> }`
— a JS array is an exotic object with index elements *and* string properties, and
Smelt's array has nowhere to put the latter, so both store seams "solve" it by
throwing the array away.

**Fix design (general).** Give `SmeltList<T>` a lazily-allocated named-property
side table, `props: Option<Rc<RefCell<Vec<(String, SmeltUnknown)>>>>` (SmeltUnknown
regardless of `T`; source-level named writes only reach erased arrays, and `None`
keeps the common case free). Then:
* `smelt_index_assign` / the `control_flow.rs` field store insert into `props`
  instead of clobbering the array;
* the array arm of erased property *reads* consults `props` after the
  index/`length` cases;
* `Object.keys`/`for…in` on an array yield index keys then `props` keys;
* structural equality on arrays keeps ignoring `props` (which is what
  `isEqualWith.spec.ts:181` asserts — JS `isEqual` compares arrays index-wise).
A thread-local `id -> props` registry beside `SMELT_FUNCTION_IDENTITIES` is a
less invasive alternative but leaks entries and splits array state across two
homes.

**Shared root:** `isEqualWith` *"should treat arrays with identical values but
different non-index properties as equal"* (`isEqualWith.spec.ts:181`, another
group) is the same defect through the static-store seam.

**Regression test shape:** runtime test — `const a = ['1']; (a as any).x = 2;`
assert `Array.isArray(a)`, `a[0] === '1'`, `a.length === 1`, `(a as any).x === 2`,
`Object.keys(a)` = `['0','x']`.

**Verdict: (a) general defect. Size M** (runtime + 3 emit seams).

---

## 3. `mergeWith` — should respect `null` returned from `customizer`  → **family A**

**Spec** `third_party/es-toolkit/src/object/mergeWith.spec.ts:63-76`

```ts
mergeWith(cloneDeep(obj), cloneDeep(source), targetValue => {
  if (targetValue === null) { return null; }
  return undefined;
})   // JS: merged === null, which is !== undefined, so target.prop = null
```

**Generated Rust** — `mergeWith_spec.rs:275, 285-296`

```rust
let mut _smelt_tmp_7: ::std::rc::Rc<dyn Fn(&SmeltUnknown) -> ()> = ...;
_smelt_tmp_7 = ::std::rc::Rc::new(|closure_arg_0: &SmeltUnknown| {
    let _smelt_tmp_1: bool = matches!(closure_arg_0.clone(), SmeltUnknown::Null);
    if _smelt_tmp_1 { _smelt_tmp_2 = (); () } else { _smelt_tmp_2 = (); () }
});
... merge_with_975(..., &mut { let _smelt_adapted_callback = _smelt_tmp_7.clone();
      move |arg0, arg1, arg2, arg3, arg4| { (_smelt_adapted_callback)(arg0); SmeltUnknown::Undefined } });
```

The customizer's inferred return type is `null | undefined`, which lowers to the
**unit type** `()`: `return null` and `return undefined` both render as `()`, and
the adapter then materializes a hard-coded `SmeltUnknown::Undefined`. So
`merged === undefined` in the library, the `isPlainObject(sourceValue)` branch
runs, and `prop` becomes `{foo:'bar'}` instead of `null`.

**Root layer: type lowering (frontend-ts / hir), with the emitter seam that
commits the lie.**

* `crates/smelt-specialize/src/manifest.rs:368-370` — one `StaticType::Null`
  ("Null-like value") for both JS `null` and `undefined`.
* `crates/smelt-frontend-ts/src/lowering/specialization.rs:581` —
  `StaticType::Null => Type::None`.
* `crates/smelt-frontend-ts/src/lowering/decls/arrows.rs:423`
  (`last_return_type`): both returns have the *same* `TypeId`, so the union
  fallback (`Type::Unknown`, which would be correct here and is a genuine
  dynamic boundary — the declared param is `(…) => any`) is never taken.
* `crates/smelt-codegen-rust/src/emitter/core.rs:3862-3874` (twin at
  `3040-3050`): a `Type::None`-returning source callback adapted into an erased
  slot is emitted as `{ call; SmeltUnknown::Undefined }`. The comment says this
  exists for `cloneDeepWith`'s void customizer — correct for `void`, wrong for
  `null`.

The limitation is already documented in
`crates/smelt-frontend-ts/src/lowering/expr/operators.rs:2770-2797`:
`Literal::None` and `Literal::Undefined` stay distinct **values**, but
`Optional`/`Union` normalization (`smelt_hir::type_normalize`,
`normalize_optional_type` / `flatten_union_none`) leaves the **type** with one
empty state; `NormalizeOptions::preserve_observable_absence` exists and nothing
enables it.

**Fix design (general).** Make absence spellings distinguishable in the type
system: either add `Type::Null` alongside `Type::None`(=undefined/void) with
`StaticType` split to match, or enable `preserve_observable_absence` in the
canonical form. Then `null | undefined` is a two-valued type; `last_return_type`
sees two distinct `TypeId`s and interns `Unknown` (a real dynamic boundary — the
declared callback type is `any` — not an erasure of a knowable shape), the arrow
body keeps `SmeltUnknown::Null` vs `SmeltUnknown::Undefined`, and the
`core.rs:3862` shortcut narrows to genuine `void`.

Interim, still general and much smaller: gate the `core.rs:3862` substitution on
the source callback having no `return <null literal>` in its MIR body — it keeps
the type lie but stops materializing the wrong tag.

**Shared root:** test 4 below (`cloneDeep` `b['#b']`). Adjacent in the same
"absence has no representation" family, though not identical:
`maxBy`/`minBy`/`reduceAsync` *"returns undefined for empty"* — there the
declared `T | undefined` collapses to `T` (`maxBy_spec.rs:87`: `let result: Person;`)
so `toBeUndefined()` const-folds to `false`.

**Regression test shape:** frontend/codegen test — a callback
`x => { if (x === null) return null; return undefined; }` passed to a
`(cb: (v: any) => any) => any` helper; assert the helper observes `null` for one
input and `undefined` for the other.

**Verdict: (a) general defect. Size L** for the principled fix (canonical type
form), **S** for the gated interim.

---

## 4. `cloneDeep` — should clone instance (`expect(b['#b']).toBe(undefined)`) → **family A**

**Spec** `third_party/es-toolkit/src/object/cloneDeep.spec.ts:166`

```ts
expect(b['#b']).toBe(undefined);   // JS: '#b' is not a string key; private fields are not properties
```

**Generated Rust** — `cloneDeep_spec.rs:469-470`

```rust
_smelt_tmp_11 = SmeltUnknown::Undefined;                       // the expected value
_smelt_tmp_12 = !(SmeltUnknown::Null.clone() == _smelt_tmp_11); // the actual value, const-folded
```

The read `b['#b']` on the class-typed receiver `b: A` correctly resolves to "no
such property", but is emitted as `SmeltUnknown::Null`, and `toBe` is strict
equality, so `null !== undefined` fails. Everything else about this test already
works (the private `#b` is *not* leaking as a string key — the value is simply
tagged `null` instead of `undefined`).

**Root layer: codegen-rust emitter.**
`crates/smelt-codegen-rust/src/emitter/place.rs:483` — the fallback arm of the
`ExprKind::Index` read:

```rust
_ => Ok(self.null_value_text()),
```

reached for a `Type::Class` receiver with no index signature.
`null_value_text` (`emitter/coercion.rs:1716`) is documented as *"the canonical
boxed 'no value' … a `None` return, an absent field, the default of an erased
target"* — conflating JS `null` with JS absence, i.e. the same family-A root
seen from the emitter end.

**Fix design (general).** An *absent property* must read as
`SmeltUnknown::Undefined`; `SmeltUnknown::Null` is only ever an explicit source
`null`. Split the helper: keep `null_value_text()` for a `Type::None` value that
really is source `null`, and add `undefined_value_text()` for absence, then use
it at `place.rs:483` and at the sibling absence sites in `coercion.rs`
(`1305`, `1505`, `1514` — each needs classifying as "None-typed value" vs
"missing"). This is the emitter half of family A and can land independently.

**Shared root:** test 3 (`mergeWith`).

**Regression test shape:** runtime test — read an unmodeled bracket key off a
class instance; assert `typeof v === 'undefined'` and `v !== null`.

**Verdict: (a) general defect. Size S.**

---

## 5. `cloneDeep` — should clone String objects

**Spec** `third_party/es-toolkit/src/object/cloneDeep.spec.ts:445-452`

```ts
const strObj = new String('es-toolkit');
const cloned = cloneDeep(strObj);
expect(cloned).not.toBe(strObj);        // JS: distinct wrapper OBJECTS
expect(cloned).toBeInstanceOf(String);
```

**Generated Rust** — `cloneDeep_spec.rs:1331-1338`

```rust
let str_obj: String = "es-toolkit".to_owned();
let _smelt_tmp_2: String = match (clone_deep_468(SmeltUnknown::String((str_obj.clone()).into()))?) ...;
cloned = _smelt_tmp_2;
_smelt_tmp_4 = cloned != str_obj;                 // false: primitive value equality
_smelt_tmp_5 = !(_smelt_tmp_4);                   // -> assertion fires
```

`new String(x)` lowers to the **primitive** `String`, so it has no reference
identity and `not.toBe` cannot hold. Contrast the sibling test, which passes:
`new Number(42)` lowers to a boxed marker object (`cloneDeep_spec.rs:1355`)

```rust
SmeltRecord::from([("__smelt_number".to_owned(), SmeltUnknown::Bool(true)),
                   ("value".to_owned(), SmeltUnknown::Number(42.0 as f64))])
```

**Root layer: frontend-ts lowering.**
`crates/smelt-frontend-ts/src/lowering/new_expr.rs:178-180` dispatches
`new String(...)` to `string_constructor_expression`
(`new_expr.rs:772-800`), which returns the argument unchanged:

```rust
let value = self.argument(argument, body)?;
if Self::expr_ty(body, value) == ty { return Ok(value); }   // `new String(s)` === s
```

while `Number` and `Boolean` (lines 203-217) go through
`boxed_primitive_constructor_expression`. The boxed model for strings already
exists everywhere else: `smelt_stdlib::HOST_OBJECTS` registers
`boxed("String", "__smelt_string")` (`crates/smelt-stdlib/src/host_object.rs:374`)
and the prelude's `smelt_unbox_primitive`
(`crates/smelt-codegen-rust/src/lib.rs:2618`) already unwraps `__smelt_string`.
Only the constructor path is missing.

**Fix design (general).** Route `new String(...)` through
`boxed_primitive_constructor_expression(new_expr, body, "__smelt_string",
Literal::String(String::new()))`, i.e. delete the special case rather than add
one, and delete `string_constructor_expression`. Then verify the read paths that
receive a boxed string: string members (`.length`, `.toUpperCase()`,
`String.prototype` methods) must unbox first — the same `smelt_unbox_primitive`
hop `Number` already relies on — and `String(x)` **without** `new` must keep
returning the primitive (different call path, unaffected).

**Shared root:** none in this group. (The prior log's note that
`new String(x)` "lowers to a primitive" is confirmed exactly.)

**Regression test shape:** runtime test — `const s = new String('a');`
assert `typeof s === 'object'`, `s == 'a'`, `s !== 'a'`,
`new String('a') !== new String('a')`, `s.length === 1`.

**Verdict: (a) general defect. Size M** (constructor change is S; the unbox
sweep over string member reads is the bulk).

---

## 6. `clone` — should clone custom classes (`clonedPerson.greet === person.greet`)

**Spec** `third_party/es-toolkit/src/object/clone.spec.ts:93`

```ts
expect(clonedPerson.greet).toBe(person.greet);   // JS: both are Person.prototype.greet
```

**Generated Rust** — `clone_spec.rs:273`

```rust
_smelt_tmp_8 = !({ let smelt_receiver = cloned_person.clone();
    SmeltUnknown::Function(::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>|
        Ok::<SmeltUnknown, _>(smelt_receiver.greet()))) }
  .clone().same_js_key(
    &{ let smelt_receiver = person.clone();
       SmeltUnknown::Function(::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>|
           Ok::<SmeltUnknown, _>(smelt_receiver.greet()))) }));
```

Every *read* of a method reference mints a **fresh `Rc`** with no identity link,
so `same_js_key` → `smelt_same_erased_function` →
`smelt_same_function_identity` (`crates/smelt-codegen-rust/src/lib.rs:1199`)
compares two unrelated allocation addresses and answers `false`. In JS the
method lives once on the prototype, so any two reads are `===`.

**Root layer: codegen-rust emitter.**
`crates/smelt-codegen-rust/src/emitter/call_runtime.rs:2506-2552`
(`class_method_reference_text`) — and its prototype-entry twin
`crates/smelt-codegen-rust/src/class_proto.rs:200`, which builds the same shape
for `__smelt_proto_entries`.

**Fix design (general).** The identity machinery this needs already exists — the
prelude comment at `lib.rs:1122-1136` even names the precedent: *"Named function
items already dodge this through the per-item `__smelt_fn_value_<key>()`
accessor, which caches one erased value"*. Do the same per **(class, method)**:
emit one `fn __smelt_method_identity_<Class>_<method>() -> usize` returning a
lazily allocated, cached canonical id (thread-local `OnceCell`), and have both
`class_method_reference_text` and `class_proto.rs` follow the freshly built `Rc`
with

```rust
smelt_link_function_identity_key(&rc, __smelt_method_identity_Person_greet());
```

(`smelt_link_function_identity_key` is `lib.rs:1157`). The receiver capture stays
— the value is still callable — but identity becomes per-method, which is exactly
JS: `a.greet === b.greet === Person.prototype.greet` all become `true`, while
`a.greet.bind(a)` stays distinct.

**Shared root:** none in this group; it is the same *mechanism* (identity
registry) the erased-callback path already uses, so the fix carries no new
concept.

**Regression test shape:** runtime test — two instances of one class; assert
`a.m === b.m`, `a.m === C.prototype.m`, and that calling `a.m()` still observes
`a`'s fields.

**Verdict: (a) general defect. Size S/M.**

---

## 7. `toSnakeCaseKeys` — should preserve object prototype methods

**Spec** `third_party/es-toolkit/src/object/toSnakeCaseKeys.spec.ts:73-79`

```ts
const input = { userId: 1, toString: Object.prototype.toString };
const result = toSnakeCaseKeys(input);
expect(result).toHaveProperty('user_id', 1);   // passes
expect(result).toHaveProperty('toString');     // FAILS here
expect(result.toString).toBe(Object.prototype.toString);
```

Note what JS actually answers: `toSnakeCaseKeys` renames the own key to
`to_string`, so `'toString'` is satisfied **through the prototype chain** —
jest/vitest's `toHaveProperty` resolves a path with an own-or-one-prototype-level
`hasOwnProperty`, and every plain object inherits `Object.prototype.toString`.
The last line likewise reads the inherited method.

**Generated Rust** — `toSnakeCaseKeys_spec.rs:194-195, 204-207`

```rust
let _smelt_tmp_2: SmeltUnknown = SmeltUnknown::String("__smelt_proto:object".into());
let _smelt_tmp_3: SmeltRecord<String, SmeltUnknown> = SmeltRecord::from([
    ("userId".to_owned(), SmeltUnknown::Number(1.0 as f64)),
    ("toString".to_owned(), match _smelt_tmp_2.clone() {
        SmeltUnknown::Object(map) => smelt_get_object_field(&map, "toString"),
        _ => SmeltUnknown::Undefined }.clone())]);   // -> Undefined
...
_smelt_tmp_7 = { let smelt_key = "toString".to_owned(); match result.clone() {
    SmeltUnknown::Object(values) => values.contains_key(&smelt_key) || ... , _ => false } };
```

Two wrongs: (i) `Object.prototype` is the opaque sentinel string
`"__smelt_proto:object"`, so `Object.prototype.toString` reads as `Undefined`;
(ii) the `toHaveProperty` lowering is an **own-key-only** `contains_key`, with no
prototype level, so the inherited `toString` is invisible.

**Root layer: frontend-ts lowering (`Object.prototype` member reads and the
`toHaveProperty` matcher) + reflection prelude.**

* sentinel producer: `crates/smelt-codegen-rust/src/lib.rs:2064`
  (`smelt_prototype_sentinel`) and
  `crates/smelt-frontend-ts/src/lowering/stdlib/objects.rs:432-433`;
* matcher: `contains_key`-only presence test built by
  `crates/smelt-frontend-ts/src/lowering/testing/matchers.rs:1212-1250`
  ("Create a dictionary key containment expression for `toHaveProperty`");
* the reflected-prototype registry that would host real members:
  `crates/smelt-codegen-rust/src/reflection_prelude.rs:53-68`, whose comment
  warns that handing a marker a reflected prototype object makes
  `Object.create` copy its members into `__smelt_proto:` keys, "visible to the
  structural equality that library specs compare clones with" — so materializing
  `Object.prototype` as a record is the wrong shape.

**Fix design (general).** Model `Object.prototype`'s members as a **lookup
fallback**, not as stored entries — that is the JS object model, and it is one
rule for all objects, not a per-library case:

1. a prelude table of `Object.prototype` members (`toString`, `toLocaleString`,
   `valueOf`, `hasOwnProperty`, `isPrototypeOf`, `propertyIsEnumerable`,
   `constructor`), each a cached `SmeltUnknown::Function` with one stable
   canonical identity (same mechanism as fix 6) — so
   `Object.prototype.toString` reads as that value and two reads are `===`;
2. `smelt_get_object_field` (and the `"__smelt_proto:object"` sentinel arm)
   falls back to that table after own and `__smelt_proto:` lookup;
3. presence checks that JS resolves through the chain — `in`, `toHaveProperty` —
   consult the same table (JS `'toString' in {}` is `true` today too, so this
   fixes an independent latent wrong answer);
4. enumeration (`Object.keys`, `for…in`, structural equality, JSON) does **not**
   see the table, which is why it must be a fallback rather than entries.

**Shared root:** none of my tests. It does not share the `Array.isArray`
const-fold root the earlier triage attributed to `toSnakeCaseKeys`
(`blocker-logs/estk-remaining-triage.md:189`) — that concerns the array branch of
a different test.

**Regression test shape:** runtime test — `const o = {a:1};` assert
`'toString' in o`, `typeof o.toString === 'function'`,
`o.toString === Object.prototype.toString`, and `Object.keys(o)` = `['a']`.

**Verdict: (a) general defect. Size M.** Not out of scope: nothing here needs a
host capability, only the standard object prototype chain.

---

## 8. `toMerged` — should deeply merge nested objects

**Spec** `third_party/es-toolkit/src/object/toMerged.spec.ts:23-42`

```ts
const names   = { characters: [{ name: 'barney' }, { name: 'fred' }] };
const ages    = { characters: [{ age: 36 }, { age: 40 }] };
const heights = { characters: [{ height: '5\'4"' }, ...] };
expect(toMerged(toMerged(names, ages), heights)).toEqual(expected);
// expected characters: { name: 'barney', age: 36, height: '5\'4"' }  — age is the NUMBER 36
```

**Generated Rust** — `toMerged_spec.rs:132`

```rust
let _smelt_tmp_37: SmeltUnion1986 = SmeltUnion1986::from_smelt_unknown(
    (to_merged(/* names erased */, /* ages erased */)?).into_smelt_unknown());
```

with (`main.rs:7583-7599`)

```rust
pub enum SmeltUnion1986 {
    M0(SmeltRecord<String, SmeltList<SmeltRecord<String, String>>>),
    M1(SmeltRecord<String, SmeltList<SmeltRecord<String, f64>>>),
}
impl SmeltUnion1986 { fn from_smelt_unknown(value: SmeltUnknown) -> Self {
    if matches!(value, SmeltUnknown::Object(_)) { return Self::M0(/* coerce every leaf to String */); }
    Self::M1(...) } }
```

`toMerged`'s declared return `T & S` was lowered as the **union** `typeof names |
typeof ages`, and the union's recovery discriminates by `SmeltUnknown` **tag
only**: both arms are objects, so the merged value is forced into `M0` and every
leaf is *coerced to `String`* — `age: 36` becomes `"36"`. Re-erasing for the
comparison then produces `SmeltUnknown::String("36")` and `toEqual` fails.

**Root layer: frontend-ts type lowering, plus the codegen union recovery.**

* `crates/smelt-frontend-ts/src/lowering/ty/annotations.rs:168-228`
  (`ts_type_to_hir`, `TSType::TSIntersectionType`): the final fallback is
  `_ => Ok(self.ctx.krate.types.intern(Type::Union(meaningful)))`. An
  intersection is not a union — the merged value belongs to **neither** arm.
  (The `all Class|Dict` arm just above, `annotations.rs:216-226`, already does
  the right kind of thing by producing `Dict(String, Unknown)`; type parameters
  never reach it.)
* `crates/smelt-codegen-rust/src/emitter/union.rs:496-513`
  (`union_from_smelt_unknown_body`): arm selection is
  `if matches!(value, {tag pattern}) { return Self::M{i}(extract) }`, so two
  object-shaped arms are indistinguishable and the *extraction* silently
  re-types leaves instead of failing.

**Fix design (general).**
1. Lower a record/object intersection **structurally**: merge the members
   (union of fields; per-field intersection of the two field types). Here
   `Record<'characters', List<Record<string,String>>> & Record<'characters',
   List<Record<string,f64>>>` becomes `Record<'characters',
   List<Record<string, String|f64>>>` — precisely `expected`'s type
   (`SmeltRecord<String, SmeltList<SmeltRecord<String, SmeltUnknown>>>`), and the
   test's own `expected` literal already proves that spelling exists. Extend the
   `annotations.rs:216` arm to compute this instead of collapsing to
   `Dict(String, Unknown)`, and make it the fallback for the type-parameter case
   after substitution rather than `Type::Union`.
2. Harden `union_from_smelt_unknown_body`: when several arms share one
   `SmeltUnknown` tag, discriminate **structurally** (required-key presence /
   value-tag checks per arm) and, when no arm matches, keep the erased value
   rather than coercing a mismatched arm. A coercion that rewrites `36` to
   `"36"` while recovering a union is a silent data corruption independent of
   the intersection bug.

**Shared root:** none of my other tests, but (2) is a general hazard for every
multi-object-arm union in the corpus.

**Regression test shape:** frontend test pinning that `A & B` over two record
types lowers to the merged record type (not `Union`); codegen runtime test that a
two-object-arm union round-trips a value belonging to the second arm without
retyping its leaves.

**Verdict: (a) general defect. Size M** (intersection lowering M; union
discrimination S/M).

---

## Summary

| test | root family | verdict | size |
|---|---|---|---|
| `invert` should not invert inherited properties | **C** marker keys survive the erased-object → `SmeltJsMap` coercion, and JsMap own-key enumeration has no marker filter (`coercion.rs:2371`, `map.rs:823`) | (a) general defect | S |
| `merge` should behave like recursive Object.assign | **B** a JS array cannot carry non-index properties; both store seams replace the array with an object (`lib.rs:3043` `smelt_index_assign`, `control_flow.rs:535`) | (a) general defect | M |
| `mergeWith` should respect `null` from customizer | **A** `null`/`undefined`/`void` all lower to `Type::None`; adapter materializes `SmeltUnknown::Undefined` (`core.rs:3862`, `arrows.rs:423`, `specialization.rs:581`) | (a) general defect | L (S interim) |
| `cloneDeep` should clone instance (`b['#b']`) | **A** absent property read emits `SmeltUnknown::Null`, not `Undefined` (`place.rs:483` → `coercion.rs:1716`) | (a) general defect | S |
| `cloneDeep` should clone String objects | **E** `new String(x)` returns the primitive instead of the `__smelt_string` box that already exists for Number/Boolean (`new_expr.rs:772`) | (a) general defect | M |
| `clone` should clone custom classes | **F** each method-reference read mints a fresh `Rc` with no canonical identity (`call_runtime.rs:2506`, `class_proto.rs:200`) | (a) general defect | S/M |
| `toSnakeCaseKeys` should preserve object prototype methods | **G** `Object.prototype` members are not modeled, and property presence never consults the prototype chain (`lib.rs:2064`, `matchers.rs:1212`) | (a) general defect | M |
| `toMerged` should deeply merge nested objects | **D** `T & S` lowered as a union, and union recovery picks an arm by `SmeltUnknown` tag then retypes its leaves (`annotations.rs:227`, `union.rs:496`) | (a) general defect | M |

No test in this group is out of scope: none requires DOM, cross-realm `node:vm`,
Node `Buffer`, global monkey-patching, or `vi.spyOn` on host internals.

Cross-group notes:
* family **A** also owns `maxBy`/`minBy`/`reduceAsync` *"returns undefined for
  empty"* in a weaker form (the declared `T | undefined` collapses to `T`, so
  `toBeUndefined()` const-folds to `false` — `maxBy_spec.rs:87`).
* family **B** also owns `isEqualWith` *"arrays with identical values but
  different non-index properties"* (`isEqualWith.spec.ts:181`), which fails
  through the `control_flow.rs:535` static-store seam.
