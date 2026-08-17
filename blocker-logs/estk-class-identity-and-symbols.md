# es-toolkit: class-instance identity and symbol-keyed properties

Campaign follow-up to `estk-clone-and-equality.md`. Target cluster (7 tests), and
what each one actually needed. Baseline at the start of this pass: **865 passed /
194 failed**. After: **869 passed / 190 failed**, zero newly failing.

| Test | Status | Root cause |
| --- | --- | --- |
| `isEqualWith should compare objects with constructor properties …` | **fixed** | `finally` skipped on `return` |
| `cloneDeep should clone objects` | **fixed** | symbol keys lost their tag |
| `isEqualWith should compare symbol properties …` | **fixed** | symbol keys lost their tag |
| `zipWith should provide index parameter …` (not in the cluster) | **fixed** | mixed-element list concat emptied |
| `clone should clone custom classes` | open | erased method references have per-read identity |
| `cloneDeep should clone instance` | open | TS `private` fields erased away; re-inlined const arrow |
| `cloneDeep should clone read-only properties` | open | `Object.defineProperties` is unmodeled |
| `isEqualWith should compare object instances …` | open | `new F()` on a plain function is unmodeled |

## Fixed

### `finally` was skipped whenever the `try` body returned

MIR made the finalizer the *fall-through exit* of the `try` body. A `return` set a
`Return` terminator instead, so the edge was never taken and the finalizer did not
appear in the generated Rust at all.

es-toolkit `areObjectsEqual` registers each visited pair in a recursion `Map` and
clears it in a `finally`, returning from inside the `try`. The leaked entries made
a later comparison of the same pair take the "already visiting, therefore equal"
shortcut, so `isEqualWith({ constructor: [1] }, { constructor: ['1'] })` answered
`true`. The `{ x: [1] }` vs `{ x: ['1'] }` shape stayed correct only because
`a.constructor` is `undefined` there and short-circuits before anything is pushed —
which is why the defect looked like a `constructor`-key special case.

Fix: `lower_return` re-lowers every enclosing finalizer inline ahead of the
`Return`. A shared cleanup block reached by `Goto` was tried first and **rejected
by measurement**: a `return` deep inside a loop needs a jump out of that loop into
a join block, and codegen's structured reconstruction degrades that to a plain
`break`, so control falls into the code after the loop. `isEqual should return
false for arrays with different values` started failing. Inlining introduces no new
CFG shape.

### Symbol-keyed properties

`Object.getOwnPropertySymbols` returned the symbol *descriptions* as `String`s,
while the record stores the property under `"__smelt_symbol:<description>"` and a
symbol value everywhere else is `SmeltUnknown::Symbol(description)`. So
`source[syms[0]]` looked up the unprefixed string key and missed, and
`target[syms[0]] = v` created a plain string property no symbol lookup could find.
The projection now re-tags the description; the element type is `Unknown`.

Two blockers surfaced behind it:

* `[...Object.keys(o), ...getSymbols(o)]` is `List<String>` chained onto
  `List<Unknown>`. `list_concat_text` bailed on the mismatch and returned
  `Default::default()` — an **empty** list. `compat/object/mergeWith` uses exactly
  that spelling, so its whole merge loop had been iterating nothing.
* `Object.prototype.propertyIsEnumerable.call(o, s)` was read off the opaque
  `"__smelt_proto:object"` sentinel, resolved to `undefined`, and answered `null`.
  es-toolkit `getSymbols` gates every symbol on it, so the symbols were filtered
  away right after being found. It now lowers to the own-key check, which is the
  only distinction Smelt's record model can make (the only non-enumerable entries
  are internal `__smelt_*` markers, which no source key can name).

## Open, with root causes

### `clone should clone custom classes`

Only one assertion fails: `expect(clonedPerson.greet).toBe(person.greet)`.

Both sides are class *method references*, and `class_method_reference_text`
(`emitter/call_runtime.rs`) emits a fresh bound closure per read:

```rust
{ let smelt_receiver = cloned_person.clone();
  SmeltUnknown::Function(Rc::new(move |smelt_args| Ok(smelt_receiver.greet()))) }
```

`same_js_key` compares functions with `Rc::ptr_eq`, so two reads are never equal.
In JavaScript `a.greet` and `b.greet` are both `Person.prototype.greet` — one
function object.

The other three assertions already pass: `clonedPerson` is reconstructed as a
concrete `Person`, so `toEqual` holds and `toBeInstanceOf(Person)` folds to true.
`__smelt_class` carrying the class name (the `__smelt_error` treatment) is **not**
what this test needs.

Making it pass means giving an erased function value a stable identity that is not
its `Rc` allocation. The shape that fits the existing code is a side table like
`SMELT_FUNCTION_ORIGINS` / `SMELT_CALLABLE_OBJECTS`: register the wrapper under a
compile-time `"<Class>.<method>"` key and have the `Function` arms of `same_js_key`
and `smelt_unknown_structural_eq` prefer a registered identity over `ptr_eq`. Two
reasons it was not landed here:

1. It changes function equality for the whole corpus, not just method references —
   that needs its own regression pass.
2. Keyed on the `Rc` data pointer it must hold a strong clone to stop address reuse
   from handing a stale identity to an unrelated closure, which leaks one entry per
   method-reference *read*. The existing registries have the same hazard but are
   not consulted by equality, so a wrong answer there is not observable.

### `cloneDeep should clone instance`

Three independent defects, in order of how much they matter:

1. **TS `private` fields are erased away.** `class_unknown_object_text`
   (`emitter/coercion.rs`) filters `Visibility::Private`, and the frontend maps
   both `private c: number` and `#b: number` to `Visibility::Private`
   (`lowering/support.rs::visibility` and the `PrivateIdentifier` checks in
   `decls/functions.rs`). But TS `private` is a compile-time modifier only — at
   runtime `c` is an ordinary own property that clones and enumerates — while `#b`
   is a real JS private field that does not. The spec asserts exactly that split:
   `expect(b).toEqual({ props, d, c: 2 })` and `expect(b['#b']).toBe(undefined)`.
   Fixing it needs HIR to distinguish JS-private from TS-accessibility-private
   (a new `Visibility` variant, or a flag on `Field`; `Visibility` is shared with
   methods, statics, and the Python frontend, so the variant is not free).
2. **A `const` arrow is re-inlined as a fresh closure.** The expected object's `d`
   shorthand emits `Rc::new(|| 1.0)` — a new allocation, not `d.clone()` — so it
   cannot be `Rc::ptr_eq` with the instance's `d` under any of the above.
3. Same erased-function identity question as `clone should clone custom classes`.

### `cloneDeep should clone read-only properties`

`Object.defineProperties` is not recognized at all: the member read resolves on an
empty record and the call falls back to a stub returning `Null`, so `object` only
ever gets its later `object.third = 3`. `Object.defineProperty` *is* recognized
(`objects.rs::object_metadata_mutation_call`) but lowers to `Literal::None` — a
no-op. The spec's second descriptor is an accessor (`get() { return 2 }`), which
the object literal also lowers to `SmeltUnknown::Null`.

Making it pass is a feature, not a fix: `defineProperty`/`defineProperties` that
actually install properties, accessor descriptors whose getter runs on read, and
per-property enumerability. Smelt currently expresses non-enumerability only
through internal `__smelt_*` marker keys, so `enumerable: false` needs a
representation (a `__smelt_nonenum:` key prefix would follow the existing
`__smelt_proto:` pattern).

Note the interaction: while `defineProperty` stays a no-op, the `enumerable: false`
symbol in `isEqualWith should compare symbol properties` is simply never added,
which happens to be indistinguishable from the correct answer. Implementing
`defineProperty` without enumerability would *regress* that test.

### `isEqualWith should compare object instances …`

Only the second assertion fails:
`expect(isEqualWith(new Foo(), new Bar(), noop)).toBe(false)` answers `true`.

`new F()` where `F` is a plain function is unmodeled end to end. Probed: `new
ZzFoo()` (whose body is `this.a = 1`) yields an object with **no own keys**,
`Object.getPrototypeOf` of it is the empty string, and `F.prototype.a = 1` goes
nowhere. Both `new Foo()` and `new Bar()` are therefore `{}` and compare equal.
The other three assertions pass only by accident (`{ a: 1 }` vs `{}` differs in key
count).

A real fix needs constructor functions: `new f()` allocating an object, binding
`this` inside the function body, a mutable `f.prototype` object, `x.constructor`
resolving to `f`, and `isPlainObject(x)` answering false for it. That is a
structural feature well beyond this cluster and should be scoped on its own — it is
also what several other open es-toolkit failures need.

## Measurement notes

* Generated assertion messages carry no actual/expected values. Every root cause
  above was pinned with a throwaway `src/predicate/zzprobe.spec.ts` that
  `console.log`s the values, built with `smelt build` and run with
  `cargo test … zzprobe -- --nocapture`. Much faster than reading assertions.
* SmeltUnknown ratchet across both commits: avoidable erasure 35349 → 35529 (+180),
  re-snapshotted, with attribution in each commit message. No line was
  reclassified. The +87 from the `finally` fix is duplicated *existing* cleanup
  statements; the +93 from the symbol fix is `let key: SmeltUnknown;` declarations
  for property-key lists, which have no concrete Smelt type to carry (a symbol *is*
  `SmeltUnknown::Symbol`).
* The symbol fix also grew legitimate-boundary occurrences by ~24.8k, 23.8k of
  which is one shape: the `smelt_property_key` helper, which
  `property_key_to_string_text` inlines a full copy of at **every** dynamic
  `obj[key]` site. `compat/object/mergeWith`'s key loop now actually runs, so that
  one file is 2.7 MB. Hoisting the helper into the prelude and calling it would cut
  generated size substantially; it touches many existing string goldens, so it is
  worth doing as its own change.
