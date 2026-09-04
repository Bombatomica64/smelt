# es-toolkit final 45 — group: FUNCTIONS / `this` / CALLABLES

Read-only investigation. No `cargo` was run. Every claim below is verified against
the generated crate in `/home/user/smelt/third_party/es-toolkit/dist-smelt/src`, and
each root cause is additionally reproduced with the **prebuilt** `smelt` binary
(`/home/user/smelt/target/debug/smelt build`, not cargo) on a minimal 5–11 line
TypeScript probe in a scratch project. Probes are named `P1..P6` below.

Six distinct root causes cover the seven assigned tests. None of them needs a host
capability; all six are general lowering/emitter defects.

---

## R1. Property names are case-folded into the same symbol as declaration names

**Test:** `__smelt_module_intersectionWith_spec::test_intersectionwith_should_return_the_intersection_of_two_arrays_with_mapper`
**Spec:** `third_party/es-toolkit/src/array/intersectionWith.spec.ts:7`

```ts
expect(
  intersectionWith([{ foo: 1 }, { foo: 2 }], [{ foo: 1 }, { foo: 3 }], (x, y) => x.foo === y.foo)
).toStrictEqual([{ foo: 1 }]);
```

JS answers `[{ foo: 1 }]`.

### Wrong generated Rust — `dist-smelt/src/intersectionWith_spec.rs` (mapper closure)

```rust
_smelt_tmp_12 = ::std::rc::Rc::new(|closure_arg_0: &SmeltUnknown, closure_arg_1: &SmeltUnknown| {
let _smelt_tmp_2: bool = match closure_arg_0.clone() { SmeltUnknown::Object(map) => smelt_get_object_field(&map, "Foo"), _ => SmeltUnknown::Undefined }.clone().js_strict_eq(&match closure_arg_1.clone() { SmeltUnknown::Object(map) => smelt_get_object_field(&map, "Foo"), _ => SmeltUnknown::Undefined });
_smelt_tmp_2
});
```

The source property is `foo`; the emitted key is `"Foo"`. Both reads therefore return
`SmeltUnknown::Undefined`, `Undefined === Undefined` is `true` for every pair, so the
comparator accepts everything and the result is `[{foo:1},{foo:2}]`, not `[{foo:1}]`.
The elements themselves are built correctly (`SmeltRecord::from([("foo".to_owned(), 1.0)])`),
so only the *member read* is corrupted.

### Root cause (frontend-ts)

`crates/smelt-frontend-ts/src/lowering/stmt/assignments.rs:441`

```rust
let field = self.intern_source_name(member.property.name.as_str());
```

`intern_source_name` (`crates/smelt-frontend-ts/src/lowering/expr/references.rs:15`) interns
the **case-folded** key and records the original spelling:

```rust
let symbol = self.ctx.krate.symbols.intern(&camel_to_snake(name));
self.ctx.krate.names.record(symbol, name);
```

`camel_to_snake("Foo") == "foo"` (`crates/smelt-frontend-ts/src/ident.rs:4` — a leading
capital is lowercased with no `_` inserted), so a declaration named `Foo` and a property
named `foo` intern to the *same* `Symbol`. `OriginalNameTable::record`
(`crates/smelt-hir/src/symbol.rs:43`) is last-writer-wins, so whichever spelling is
lowered last owns the symbol crate-wide. The emitter then reads it back through
`Emitter::symbol_source_name` (`crates/smelt-codegen-rust/src/emitter/core.rs:4999`) at
`crates/smelt-codegen-rust/src/emitter/place.rs:158`:

```rust
"match {scrutinee} {{ SmeltUnknown::Object(map) => smelt_get_object_field(&map, {field_name:?}), _ => SmeltUnknown::Undefined }}"
```

es-toolkit declares `function Foo(value: unknown)` in `partial.spec.ts` /
`partialRight.spec.ts` and `class Foo` in `isEqualWith.spec.ts`; symbols are crate-global
(`ctx.krate.symbols`), so this poisons every erased `.foo` read in the whole crate.

**P1 (reproduced, 6 lines):**
```ts
export function readFoo(x: unknown, y: unknown): boolean {
  return (x as any).foo === (y as any).foo;
}
function Foo(value: unknown) { return value; }
export function useFoo(): unknown { return Foo(1); }
```
emits `smelt_get_object_field(&map, "Foo")`. Deleting `function Foo` (or replacing it
with `class Foo`, which goes through the non-folding `intern_type_name`) restores `"foo"`.

### Shared with

Same corruption is present in `dist-smelt/src/intersectionBy_spec.rs:42` and four places
in `dist-smelt/src/isEqualWith_spec.rs` (`:2339,2344,2346,2351` — `object1.Foo`), so the two
`isEqualWith` failures in `failures.txt` should be re-checked against this root before being
diagnosed separately.

### Verdict — (a) general defect, fixable. Size **S–M**

JavaScript property keys are case-sensitive; a Rust-identifier case fold must never
reach a key string. Fix in the frontend, one of:

1. Intern member/property names **exactly** — replace the `intern_source_name` call at
   `assignments.rs:441` (and the sibling sites `assignments.rs:2162`, `assignments.rs:2781`,
   and any other member/key interning) with `intern_exact_source_name`, which already
   exists precisely for "JavaScript object keys are case-sensitive" (`references.rs:26`); or
2. better and more general: keep `SymbolInterner` keyed on the **exact** source spelling and
   apply `camel_to_snake` only where a *Rust identifier* is rendered (i.e. in the emitter's
   name construction), so no two distinct source names can ever share a `Symbol`.

Option 2 also removes the silent, order-dependent aliasing of any pair of names that differ
only by case (`Foo`/`foo`, `URL`/`url`), which is a latent correctness hazard well beyond
this test.

Regression test shape: a fixture with `function Foo(){}` plus an erased `x.foo` read;
assert the emitted source contains `smelt_get_object_field(&map, "foo")` and not `"Foo"`.
Add a `SymbolInterner`/`OriginalNameTable` unit test asserting `Foo` and `foo` get
different symbols.

---

## R2. A user function named `negate` is replaced by a null constant (illegal name special case)

**Test:** `__smelt_module_negate_spec::test_negate_should_negate_the_given_predicate_function`
**Spec:** `third_party/es-toolkit/src/function/negate.spec.ts:7`

```ts
expect(negate(() => true)()).toBe(false);
```

JS answers `false`.

### Wrong generated Rust — `dist-smelt/src/negate_spec.rs`

```rust
_smelt_tmp_2 = SmeltUnknown::Null;
_smelt_tmp_3 = { let smelt_function_value = _smelt_tmp_2.clone(); … if let Some(smelt_function) = smelt_callable { … } else { SmeltUnknown::Null } };
_smelt_tmp_4 = SmeltUnknown::Bool(false);
_smelt_tmp_5 = !(_smelt_tmp_3 == _smelt_tmp_4);
```

`negate(() => true)` is the constant `SmeltUnknown::Null`; calling it yields
`SmeltUnknown::Null`, which is not `Bool(false)`. `negate_603` (the real lowered function
in `dist-smelt/src/negate.rs`) is **never called anywhere in the crate** — including from
library code: `dist-smelt/src/reject.rs:14` lowers es-toolkit's own
`filter(source, negate(iteratee(predicate)))` to

```rust
let _smelt_tmp_5: SmeltList<SmeltUnknown> = { let smelt_l: SmeltList<_> = (filter_498(source.clone(), Some(SmeltUnknown::Null))?) … };
```

i.e. `reject` silently passes a null predicate.

### Root cause (frontend-ts) — a function-name special case

`crates/smelt-frontend-ts/src/lowering/stdlib/objects.rs:807` `lodash_negate_call`, wired
into the stdlib dispatch table at `crates/smelt-frontend-ts/src/lowering/stdlib/call_dispatch.rs:1251`
(which runs **before** user-item resolution):

```rust
if callee.name != "negate" || !self.imports.is_value("negate") {
    return Ok(None);
}
…
Ok(Some(body.push_expr(Expr { kind: ExprKind::Literal(Literal::None), ty /* fn(unknown)->bool */, span })))
```

Any call to an *imported identifier literally named `negate`* is replaced by `null` typed
as `fn(unknown) -> bool`. Because the type is a function type, `typeof negate(…)` still
folds to `"function"` (`negate_spec.rs` line 1: `"function".to_owned() != "function".to_owned()`),
which is why the first assertion passes and only the invocation fails.

**P2 (reproduced, 2 files, 8 lines total):** a local `negate.ts` with es-toolkit's exact
body plus `const f = negate(() => true); return f();` emits

```rust
let f = { let smelt_default_callback: ::std::rc::Rc<dyn Fn(&SmeltUnknown) -> bool> = ::std::rc::Rc::new(move |arg0: &SmeltUnknown| -> bool { false }); smelt_default_callback };
let _smelt_tmp_1: bool = (f)(&(SmeltUnknown::Null));
```

Renaming the export to `negateX` (nothing else changed) emits a real
`negate_x_1(SmeltUnknown::Function(…))` call. The name is the only trigger.

### Verdict — (a) general defect, fixable. Size **S**

This is a direct violation of CLAUDE.md "Type lowering — WE DO NOT DO SPECIAL CASES FOR
CODE". Delete `lodash_negate_call` and its dispatch entry; the ordinary imported-item
path already lowers `negate` correctly (proved by the `negateX` rename). Audit the
adjacent name-keyed entries in the same table for the same hazard — `lodash_has_call`
(`objects.rs:844`, folds `_.has(o, p)` to `false`), `lodash_fp_curried_call`
(`objects.rs:878`, folds ten `fp.*` names to `null`), `lodash_for_each_call` — each of
these will shadow a user export of the same name.

Regression test shape: a fixture exporting a local `negate` (and `has`) with a real body;
assert the call site emits a call to the lowered item, not `SmeltUnknown::Null` /
`smelt_default_callback`.

---

## R3. In a closure body, `!unknownValue` is lowered to a `typeof === "boolean"` tag check

**Test:** same test as R2 (second, independent defect on the same path)
**Spec:** `negate.spec.ts:8` / `negate.ts:14`

```ts
return ((...args: any[]) => !func(...args)) as F;
```

### Wrong generated Rust — `dist-smelt/src/negate.rs` (inside the returned closure)

```rust
let _smelt_tmp_2: SmeltUnknown = { …call func… };
let _smelt_tmp_3: bool = matches!(_smelt_tmp_2.clone(), SmeltUnknown::Bool(_));
let _smelt_tmp_4: bool = !(_smelt_tmp_3);
```

`matches!(x, SmeltUnknown::Bool(_))` is `typeof x === "boolean"`, not JS truthiness. So
`!func(...)` answers `false` whenever `func` returns *any* boolean — including `false`,
where JS answers `true`. (`negate(() => false)()` would be `false`.) The compat copy of
the same source, `dist-smelt/src/negate_1.rs`, lowered through the non-callback path and is
**correct**:

```rust
let _smelt_tmp_4: bool = match (…call…).clone() { SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), … => true };
let _smelt_tmp_5: bool = !(_smelt_tmp_4);
```

The same wrong shape appears in `negate_spec.rs`'s `filter` callback
(`matches!(_smelt_tmp_3.clone(), SmeltUnknown::Bool(_))`).

### Root cause (frontend-ts, callback lowering)

`CallbackExprKind` (`crates/smelt-hir/src/expr/call.rs:45`) has **no** cast/truthiness
variant, so the callback path substitutes a tag check:

* `crates/smelt-frontend-ts/src/lowering/callbacks/body_lowering.rs:615-623`
* `crates/smelt-frontend-ts/src/lowering/callbacks/dispatch.rs:415-425`

both emit, for an `unknown`-typed operand in boolean position:

```rust
CallbackExprKind::UnknownIs { value: …, kind: UnknownKind::Bool }
```

which the emitter renders through `Emitter::tag_check_raw`
(`crates/smelt-codegen-rust/src/emitter/coercion.rs:2190`, `UnknownKind::Bool => "SmeltUnknown::Bool(_)"`).
The correct node exists on the ordinary path: `ExprKind::PrimitiveCast { op: ToBool }`,
emitted by `crates/smelt-codegen-rust/src/emitter/types.rs:278-284` as the full JS
truthiness match.

### Verdict — (a) general defect, fixable. Size **M**

Add a truthiness node to the callback IR (`CallbackExprKind::PrimitiveCast { op, value }`,
or a narrow `Truthy { value }`), lower it in `crates/smelt-mir` alongside the existing
`CallbackExprKind` arms, and emit it with the same helper the non-callback `ToBool` cast
uses. Replace both `UnknownIs { kind: Bool }` uses above with it. `UnknownIs { Bool }`
should then remain reachable only from a real `typeof x === 'boolean'`.

Regression test shape: `(x: unknown) => !x` inside a callback position; assert the emitted
closure body contains the truthiness `match` and **not** `matches!(…, SmeltUnknown::Bool(_))`;
plus a runtime fixture asserting `!false === true`, `!0 === true`, `!"" === true`.

---

## R4. A plain function invoked as `obj.method(args)` never receives `obj` as `this`

**Tests:**
* `__smelt_module_memoize_spec::test_memoize_should_use_this_context_for_resolver_function` (`memoize.spec.ts:42`)
* `__smelt_module_throttle_spec::test_throttle_should_preserve_this_context_when_called_as_a_method` (`throttle.spec.ts:160`)

```ts
// memoize.spec.ts:36-42
const fn = function (a: number) { return (a + this.b + this.c) as number; };
const memoized = memoize(fn);
const object = { memoized: memoized, b: 2, c: 3 };
expect(object.memoized(1)).toBe(6);

// throttle.spec.ts:152-160
const obj = { msg: 'hello world', logWithThrottle: throttle(function (this: any) { capturedMsg = this?.msg; }, throttleMs) };
obj.logWithThrottle();
expect(capturedMsg).toBe('hello world');
```

JS answers `6` and `'hello world'`.

### Wrong generated Rust

`dist-smelt/src/memoize_spec.rs`:

```rust
_smelt_tmp_7 = { let smelt_source_value = object.get(&"memoized".to_owned()).unwrap_or(SmeltUnknown::Undefined).clone(); … smelt_callback };
_smelt_tmp_8 = (_smelt_tmp_7)(1.0);
```

`dist-smelt/src/throttle_spec.rs`:

```rust
_smelt_tmp_6 = { let smelt_source_value = obj.get(&"logWithThrottle".to_owned()).unwrap_or(SmeltUnknown::Undefined).clone(); … };
_smelt_tmp_7 = (_smelt_tmp_6)();
```

In both cases the member expression is lowered to a **field read that produces a bare
callable**, then invoked. The receiver (`object` / `obj`) is evaluated only to read the
property and is then discarded — no `smelt_push_this` guard is installed. The callee bodies
read `smelt_this()`, which still holds the ambient `SmeltUnknown::Undefined`, so
`this.b`/`this.msg` are `Undefined`: memoize computes `1 + NaN + NaN` and throttle captures
`None`.

### How the `this` channel works today (verified)

* Runtime: `SMELT_THIS: RefCell<SmeltUnknown>` thread-local, `smelt_push_this(receiver) -> SmeltThisGuard`
  (RAII restore on drop) and `smelt_this()` — emitted in
  `crates/smelt-codegen-rust/src/lib.rs:2836-2858`, gated on `needs_this_channel`
  (`lib.rs:557`, true when the MIR contains `Rvalue::ThisRead | Rvalue::BindThis`).
* Erasure: `smelt_bind_this(callee, receiver)` (`lib.rs:2861-2880`) wraps a
  `SmeltUnknown::Function` **and** a callable OBJECT (rebuilding the bag with a bound
  `__smelt_call`), and `SmeltErasedFunction::smelt_bind_this` (`lib.rs:2723-2735`) does the
  typed-value equivalent.
* `SmeltErasedFunction { callback, length, object }` (`lib.rs:2686`): **`object` is not a
  receiver slot.** Its own comment (`lib.rs:2703-2706`) says `object` is the callable's own
  JavaScript property bag — "what makes an erased callable erase as an OBJECT carrying
  `__smelt_call`". There is no receiver field anywhere on the callable representation; the
  receiver lives only in the dynamically scoped `SMELT_THIS` slot for the duration of a call.
* Frontend: `ModuleBuilder::bind_this_receiver`
  (`crates/smelt-frontend-ts/src/lowering/callbacks/closures.rs:1425`) produces
  `ExprKind::BindThis { callee, receiver }`, keeping the callee's own type. It is reached
  **only** from the explicit `fn.call(...)` / `fn.apply(...)` / `.bind(...)` lowerings
  (`callbacks/closures.rs:1366` `callback_apply_method_to_body_expr` and its `call`
  sibling). It elides the bind for a `null`/`undefined` receiver and refuses concrete
  callable *structs* (documented there as a known gap).
* Emitter: `Rvalue::BindThis` at `crates/smelt-codegen-rust/src/emitter/call_runtime.rs:242`;
  the typed form at `call_runtime.rs:290` wraps the callee in a closure that opens with
  `let _smelt_this_guard = smelt_push_this(smelt_bound_this.clone());`.

So the channel is complete and correct — nothing installs a receiver for **ordinary method-call
syntax**. Grepping `smelt_push_this` across the emitter confirms only two install sites, both
`bind`/`call`/`apply`.

Both library sides already forward correctly once a receiver exists:
`memoize.ts:89` is `fn.call(this, arg)`, `throttle.ts:61,74,81` are `func.apply(this, args)` /
`debounced.apply(this, args)`, and `dist-smelt/src/throttle.rs` / `memoize.rs` contain real
`smelt_bind_this(...)` / `smelt_this()` pairs. The *only* missing link is the call site.

**P3 (reproduced, 5 lines):**
```ts
const fn = function (a: number) { return a + (this as any).b; };
const obj = { m: fn, b: 2 };
return obj.m(1);
```
emits `let _smelt_tmp_6: SmeltUnknown = (_smelt_tmp_5)(1.0);` with no `smelt_push_this`.

### General mechanism that would fix both

A method call is a call **plus** a receiver. The general rule: whenever a call's callee is a
member expression `recv.m(...)` (and the callee is not a static/class-method dispatch that
already binds its own receiver, and not a `Type::Function` value read from a *local*), the
frontend must route the callee through the existing `bind_this_receiver(callee, receiver)`
before building the call — i.e. lower `recv.m(a)` as
`ClosureCall { callee: BindThis { callee: Field { recv, m }, receiver: recv }, args: [a] }`.
That reuses `ExprKind::BindThis`, `Rvalue::BindThis`, `smelt_push_this` and
`smelt_bind_this` unchanged, needs no new IR node, and automatically covers callable
objects (`smelt_bind_this`'s `SmeltUnknown::Object` arm) — which is exactly what throttle's
`DebouncedFuncLeading` erases to.

Two care points:
* the receiver must be evaluated **once** and shared between the property read and the
  bind (three-address MIR already gives this: the member lowering at
  `assignments.rs:425` computes `receiver` into its own expr before the field read);
* arrow functions must not be affected — they capture `this` lexically and never read
  `smelt_this()` at call time, so binding around them is inert;
* prefer emitting the bind only when the callee's static type can actually observe `this`
  (i.e. `Type::Function | Unknown | TypeParam | Union` — exactly `bind_this_receiver`'s
  existing guard), so no typed call ABI changes and `needs_this_channel` stays off for
  programs that never mention `this`.

### Verdict — (a) general defect, fixable. Size **M**

Regression test shape: `const o = { m: function(){ return (this as any).v; }, v: 7 }; o.m()`
returns `7`; a second fixture where the property holds a callable object
(`{ m: throttle(f, 1) }`) asserts `smelt_push_this` appears around the invocation; a third
asserts an arrow-valued property is unaffected and that a program with no `this` still emits
no `SMELT_THIS`.

---

## R5. `new f(...)` on a function value is lowered to a plain call; `x instanceof f` folds to `false`

**Tests:**
* `__smelt_module_partial_spec::test_partial_partial_ensures_new_par_is_an_instance_of_func` (`partial.spec.ts:67`)
* `__smelt_module_partialRight_spec::test_partialright_partialright_ensures_new_par_is_an_instance_of_func` (`partialRight.spec.ts:65`)

```ts
function Foo(value: unknown) { return value && object; }
const object = {};
const par = partial(Foo);
expect(new par() instanceof Foo).toBe(true);
expect(new par(true)).toBe(object);
```

### What JS requires

`new par()`: (1) allocate an object whose `[[Prototype]]` is `par.prototype` — and
`partialImpl` sets `partialed.prototype = Object.create(func.prototype)`
(`src/function/partial.ts:801-803`), so the chain reaches `Foo.prototype`; (2) call
`partialed` with that object as `this`; `func.apply(this, [])` returns `undefined && object`
= `undefined`; (3) a constructor whose body returns a non-object yields the allocated
object. Hence `new par() instanceof Foo === true`. `new par(true)` returns `object`
(a real object return wins), which is the second assertion.

### Wrong generated Rust — `dist-smelt/src/partial_spec.rs`

```rust
_smelt_tmp_8 = false != true;
if _smelt_tmp_8 {
return Err::<_, Box<dyn std::error::Error>>(smelt_throw(SmeltUnknown::String("expect(...).toBe(...) failed: expect(new par() instanceof Foo).toBe(true) …")));
```

`new par() instanceof Foo` is **constant-folded to `false` at compile time** — `new par()`
is not even evaluated. The next assertion shows what `new` on a function value does become:

```rust
_smelt_tmp_9 = par.call(SmeltList::from(vec![SmeltUnknown::Bool(true)]));
```

a plain invocation: no allocation, no prototype link, no `this`, no
"return the new object unless the body returned an object" rule. (`new par(true)` passes
only by luck, because `Foo` happens to return an object.)

Also broken, one layer down — `dist-smelt/src/partial_1.rs` (`partial_impl`):

```rust
let _smelt_tmp_6: bool = match match ((*smelt_capture_func.borrow())).clone() { SmeltUnknown::Object(map) => smelt_get_object_field(&map, "prototype"), _ => SmeltUnknown::Undefined }.clone() { … };
if _smelt_tmp_6 {
_smelt_tmp_7 = smelt_object_from_prototype(…);
let _ = _smelt_tmp_7;
```

`func` is a `SmeltUnknown::Function`, so the `prototype` read matches only the `Object`
arm and yields `Undefined` → falsy → the branch never runs; and even inside it the assignment
`partialed.prototype = …` is discarded (`let _ = _smelt_tmp_7;`), because
`SmeltErasedFunction` has no property slot for it (`object` is `None` here).

### Root cause

Two cooperating layers, both frontend-ts:

1. `crates/smelt-frontend-ts/src/lowering/new_expr.rs:397` `new_through_value_expression`
   lowers `new f(args)` on a function-valued binding to `ExprKind::ClosureCall` — its own
   doc comment states the intent ("`new ctor(args)` is just an indirect call through that
   callable value"). That is correct for a *class* constructor value and wrong for a plain
   function: it drops the object allocation, the receiver, and the return-value rule.
2. `crates/smelt-frontend-ts/src/lowering/guards.rs:23` `instanceof_expression`, lines
   ~150-170, folds the predicate to `false` for a function-valued target:

```rust
// Smelt's runtime never constructs closure values with `new`, so no value
// can be an instance of a plain function: the check is truthfully `false`.
let target_is_function_value = self.scope.is_bound(class_text)
    || self.scope.has_callback(class_text)
    || self.items.get(class_text).is_some_and(|&item| matches!(self.item_ref(item), smelt_hir::Item::Function(_)));
if target_is_function_value { /* Literal::Bool(false) */ }
```

The comment is self-consistent (given (1) the fold is *currently* true), but the premise
"Smelt's runtime never constructs closure values with `new`" is the defect, not a fact
about JavaScript: every non-arrow function is a constructor in JS.

**P4 (reproduced, 5 lines):** `function Ctor(this:any){this.x=1}; const par = function(this:any){ return (Ctor as any).apply(this,[]) }; return (new (par as any)()) instanceof Ctor;`
emits `let _smelt_tmp_5: bool = false;` and lowers `new par()` to a bare
`(smelt_function)(vec![])`.

### Verdict — (a) general defect, fixable. Size **L**

This is core JS semantics, not a host capability, so it is in scope. It needs three pieces,
each general:

1. **Functions get a `prototype` object.** Give the callable representation a real
   `prototype` property: a non-arrow function value, when erased, must answer
   `smelt_get_object_field(f, "prototype")` with a fresh object carrying `constructor: f`.
   The plumbing already exists — `SmeltErasedFunction::object` is the own-property bag and
   `SMELT_CALLABLE_PROPERTIES` (`lib.rs:2708`) is the side registry for properties on a
   still-typed callable; `smelt_function_method` (`lib.rs:2600`) is the precedent for
   resolving a universal function member on a `SmeltUnknown::Function` receiver. Writes
   (`partialed.prototype = …`) must land in that same bag instead of being discarded.
2. **A real construct operation.** Replace the `ClosureCall` in `new_through_value_expression`
   with a distinct `ExprKind`/`Rvalue::Construct { callee, args }` whose runtime helper
   performs JS `[[Construct]]`: allocate `SmeltObject` with `__proto__` set from
   `callee.prototype`, run the callee under `smelt_push_this(new_object)`, and return the
   callee's result if it is an object, else the allocated object. The `__proto__` machinery
   is already modeled (`smelt_proto_accessor`, `lib.rs:2076`; `smelt_object_from_prototype`,
   used by `partial_impl`; `new_expr.rs:718` comments on the same marker), so this is
   composition rather than new runtime concepts.
3. **`instanceof` walks the prototype chain.** Delete the `target_is_function_value` fold in
   `guards.rs` and lower to a runtime predicate that walks `value.__proto__` comparing
   against `target.prototype`. Keep the existing `InstanceOf`/marker path for declared
   classes and stdlib targets; only the "target is a function value" case changes from
   *fold false* to *runtime chain walk*.

Regression test shape: `function C(){}; const o = new (C as any)(); expect(o instanceof C).toBe(true)`;
`function C(){ return {tag:1} }; expect((new (C as any)()).tag).toBe(1)` (object return wins);
`function C(this:any){ this.x=1 }; expect((new (C as any)()).x).toBe(1)` (receiver installed);
and a wrapper case matching partial: `const w = function(this:any){ return (C as any).apply(this, []) }; (w as any).prototype = Object.create(C.prototype); expect(new (w as any)() instanceof C).toBe(true)`.

Both partial tests share this root exactly; `partialRight_spec.rs` has the identical
`false != true` fold.

---

## R6. An array-literal spread whose item type is a callee's out-of-scope type parameter emits an EMPTY list

**Test:** `__smelt_module_sumBy_spec::test_sumby_function_ensures_that_adding_the_sums_of_two_arrays_equals_the_sum_of_their_concatenation`
**Spec:** `third_party/es-toolkit/src/math/sumBy.spec.ts:21`

```ts
const array1: Array<{ a: number }> = [];
const array2 = [{ a: 1 }, { a: 2 }, { a: 3 }];
expect(sumBy(array1, x => x.a) + sumBy(array2, x => x.a)).toBe(sumBy([...array1, ...array2], x => x.a));
```

JS answers `6 === 6`.

### Wrong generated Rust — `dist-smelt/src/sumBy_spec.rs`

```rust
_smelt_tmp_12 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<SmeltUnknown> = vec![]; smelt_list_items }));
_smelt_tmp_13 = Into::<SmeltList<_>>::into({ let smelt_l: SmeltList<_> = (SmeltList::new(Vec::<SmeltUnknown>::new())).clone().into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| value).collect::<Vec<_>>()) });
_smelt_tmp_14 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<SmeltUnknown> = vec![]; smelt_list_items }));
_smelt_tmp_15 = Into::<SmeltList<_>>::into({ let smelt_l: SmeltList<_> = (SmeltList::new(Vec::<SmeltUnknown>::new())).clone().into(); SmeltList::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| value).collect::<Vec<_>>()) });
_smelt_tmp_16 = Into::<SmeltList<_>>::into(_smelt_tmp_13.borrow().iter().cloned().chain(_smelt_tmp_15.borrow().iter().cloned()).collect::<Vec<_>>());
```

`_smelt_tmp_13` is the spread of `array1` and `_smelt_tmp_15` the spread of `array2`, and
**both source lists are replaced by `SmeltList::new(Vec::<SmeltUnknown>::new())`** — the
`Type::List` default value. So `[...array1, ...array2]` is empty, the right-hand
`sumBy(...)` is `0`, and `6 != 0`. `array1`/`array2` themselves are lowered correctly
(the two left-hand `sum_by_674(array1.clone(), …)` calls are fine).

### Root cause

HIR is correct. `dump-mir` on **P5** (below) shows:

```
%0 user array1: List<Dict<String, Float>>
%7 temp: List<T>
%8 = list_concat move %0, move %7
```

The array literal's list type is `List<T>` — `T` is **sumBy's own type parameter**, adopted
from the argument's contextual type hint, and it is not in scope at the call site.
`ModuleBuilder::array_expression_with_spread` /
`ModuleBuilder::array_spread_item_type` /
`ModuleBuilder::list_expr_from_spread_value`
(`crates/smelt-frontend-ts/src/lowering/expr/operators.rs:2868`, `:3027`) build
`ListConcat { left: array1, right: ListLit([]) }` at that `List<T>`.

The emitter cannot relate `List<Dict<String,Float>>` to `List<T>`:
`Emitter::list_concat_text` / `Emitter::concat_result_list_ty`
(`crates/smelt-codegen-rust/src/emitter/list.rs:220` and `:314`) have two silent
last-resort fallbacks for a pair they cannot type —

```rust
return Ok("Default::default()".to_owned());
```

(twice, at the "mixed element types" branch and the non-`List` branch) — and
`default_value(List(item))` is `SmeltList::new(Vec::<item>::new())`
(`crates/smelt-codegen-rust/src/emitter/types.rs:1384`). An empty list is emitted where a
concatenation was meant.

**P5 (reproduced, 11 lines):** a local generic
`sumBy<T>(items: readonly T[], getValue: (e: T, i: number) => number)` plus
`sumBy([...array1, ...array2], x => x.a)` reproduces the generated text **byte-for-byte**.
Removing only the call (`const merged = [...array1, ...array2]`, no contextual hint) emits
the correct

```rust
let _smelt_tmp_9: SmeltList<SmeltRecord<String, f64>> = Into::<SmeltList<_>>::into(array1.borrow().iter().cloned().chain(_smelt_tmp_8.borrow().iter().cloned()).collect::<Vec<_>>());
```

so the trigger is exactly "item type hint is the callee's type parameter". A further data
point on how load-bearing the silent fallback is: in a crate where `List<Unknown>` is not
otherwise interned, the same input **hard-errors** with
`EmitError { message: "type table does not contain literal operand type List(TypeId(10)) at crates/smelt-codegen-rust/src/emitter/list.rs:350" }`
— i.e. the concat path genuinely cannot type this operand pair, and es-toolkit only avoids
the error because `List<Unknown>` happens to exist there.

### Shared with

The same empty-list text appears in library modules — `dist-smelt/src/xorWith.rs:62,90`
(where `unionWith(arr1, arr2, cmp)`'s packed argument list is emitted empty),
`xorBy.rs` (4×), `omit_1.rs` (2×), `omitBy_1.rs`, `pickBy_1.rs`. Any remaining
`omit`/`pick`/`xor`-family failure should be checked against this root first.

### Verdict — (a) general defect, fixable. Size **M**

Two changes, both general:

1. **Frontend (the actual fix).** A contextual type hint must be *instantiated* at the call
   site before it is used as a literal's item type: in `array_spread_item_type`, if the hint's
   item type is a `Type::TypeParam` that is not in scope in the current function, drop the
   hint and unify the pieces' own item types instead (falling back to `Unknown`). A hand-written
   port would never give a caller-side array the callee's un-substituted `T`.
2. **Emitter (make the class of bug impossible).** Replace both
   `return Ok("Default::default()".to_owned())` fallbacks in `list_concat_text` with an
   `EmitError`. Silently substituting an empty collection for a concatenation the emitter
   cannot type is how this stayed invisible; a build error is the correct outcome and the
   `list.rs:350` behaviour above shows the same input already errors on a neighbouring path.

Regression test shape: the P5 fixture as a frontend/MIR test asserting the concat locals
carry `List<Dict<String,Float>>` (no `List<T>`); an emitter test asserting the generated
concat text contains `array1.borrow().iter().cloned().chain(`; an emitter unit test
asserting a genuinely untypable `ListConcat` returns `Err`, not an empty list; and a runtime
fixture asserting `sumBy([...a, ...b], f) === sumBy(a, f) + sumBy(b, f)`.

---

## Summary

| test | root family | verdict | size |
|---|---|---|---|
| `intersectionWith … with mapper` | R1 property name case-folded into a declaration's symbol (`intern_source_name` + `camel_to_snake` + last-writer-wins `OriginalNameTable`) | (a) fixable | S–M |
| `negate should negate the given predicate function` | R2 `lodash_negate_call` name special case replaces the user's `negate` with `null` (primary) | (a) fixable | S |
| `negate should negate the given predicate function` | R3 callback-body `!unknown` lowered to `UnknownIs{Bool}` (typeof) instead of a `ToBool` truthiness cast (secondary, same test) | (a) fixable | M |
| `memoize should use this context for resolver function` | R4 `obj.m(args)` installs no `this` receiver | (a) fixable | M |
| `throttle should preserve this context when called as a method` | R4 (same root) | (a) fixable | M |
| `partial partial ensures new par is an instance of func` | R5 `new f()` on a function value = plain call; `instanceof <function value>` folded to `false`; functions have no `prototype` | (a) fixable | L |
| `partialRight partialright ensures new par is an instance of func` | R5 (same root) | (a) fixable | L |
| `sumBy … adding the sums of two arrays equals the sum of their concatenation` | R6 spread at a callee-`T` item type → `Default::default()` empty list in `list_concat_text` | (a) fixable | M |

No test in this group is out of scope: none requires DOM, cross-realm `node:vm`, Node
`Buffer`, global monkey-patching, or `vi.spyOn` on host internals.

### Cross-group leads produced here

* R1 also corrupts `intersectionBy_spec.rs` and `isEqualWith_spec.rs` (`.Foo` reads) — check
  the two `isEqualWith` failures against it.
* R2's sibling name special cases (`lodash_has_call`, `lodash_fp_curried_call`,
  `lodash_for_each_call` in `stdlib/objects.rs`) shadow user exports named `has`, `forEach`,
  `fp.*` the same way.
* R6 also empties packed argument lists in `xorWith.rs`, `xorBy.rs`, `omit_1.rs`,
  `omitBy_1.rs`, `pickBy_1.rs`.
* R2/R6 both show the same anti-pattern: a lowering/emit fallback that substitutes a
  *plausible value* (`null`, an empty list) for something it cannot represent. Those
  fallbacks turn compile-time gaps into silent wrong answers and are worth an audit of
  their own (`grep -n 'Literal::None' crates/smelt-frontend-ts/src` has 93 hits).
