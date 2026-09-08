# H43 — a method assigned at runtime must be a callable field

Round 15 note, **landed in round 16**. 2 slice errors cleared (`E0615` on
`SmartRouter<T>::match_`); slice 14 -> 12. What shipped is option B below, the
exact-but-narrow rule, per the coordinator ruling. See "What landed" at the end.

## The shape

`third_party/hono/src/router/smart-router/router.ts:46`:

```ts
match(method: string, path: string): Result<T> {
  for (…) {
    res = router.match(method, path)
    …
    this.match = router.match.bind(router)   // ← replaces its own method
  }
}
```

`SmartRouter` picks a concrete router on the first request and then rewrites its
own `match` so later requests skip the selection loop. Generated Rust:

```
error[E0615]: attempted to take value of method `match_` on type `&SmartRouter<T>`
    self.match_ = SmeltUnknown::Function(…);
```

The write side already emits a field assignment. The class emits `match_` as an
inherent method. Nothing reconciles the two.

## The target representation already exists

This is the part that makes H43 cheaper than it looks. Codegen can already emit
a class member as a callable field — the slice contains one:

```rust
struct __smelt_anon_class_3050Inner<T> {
    name: String,
    _middleware: Option<SmeltRecord<String, …>>,
    match_: ::std::rc::Rc<dyn Fn(PatternRouter<T>, String, String) -> SmeltUnknown>,
    …
}
```

That anon class got the field because its `match` arrived as a *function-typed
property* rather than as a method declaration. So the emitter needs no new
vocabulary: it needs the frontend to lower an assigned method as a
function-typed property initialised to the method's own body, and the existing
path does the rest. Expect the diff to be mostly in `smelt-frontend-ts`, not in
`smelt-codegen-rust`.

## The pre-pass — and yes, it needs a new fact

The model named for this is `scan_written_host_globals`
(`crates/smelt-frontend-ts/src/lowering.rs:495`): a per-file AST scan, unioned
across every source by the transpiler *before* lowering, parked on
`HirCtx::written_host_globals`. It works because `globalThis.Foo = …` has a
syntactically fixed receiver and `Foo` is checked against a static registry —
no type resolution needed.

**An assigned method does not have that property.** From a pure AST pre-pass,
`router.match = …` gives the member name and nothing about which class `router`
is. Three ways out, and they are not equivalent:

| | rule | precision | cost |
| --- | --- | --- | --- |
| A | record assigned member *names*; any class method with a recorded name becomes a field | over-approximates: an unrelated class with a `match` method also converts | pre-pass only, no new fact |
| B | record `this.<name> = …` inside a class body only (receiver known syntactically) | exact for hono, misses "externally through an instance" | pre-pass only, no new fact |
| C | record `(class, member)` pairs | exact | needs type resolution, so it cannot be a pre-pass |

The ruling asked for the general rule — "assigned anywhere in the crate (own
body, subclass, or externally through an instance)" — which is C. C cannot run
before lowering, because the receiver's class is only known once types are
resolved; and codegen needs the answer *before* it emits the class, which may
be emitted before the assigning function is lowered. So C is **a new HIR fact**:
lower the crate, collect the assigned `(class, member)` pairs, then decide each
class's member representation from that. That is a second pass over HIR, not a
scan bolted onto the existing pre-pass, and it is the thing I am flagging before
building it.

A is a pre-pass and needs no new fact, but it converts methods on classes that
never get assigned. Every such conversion turns a direct static call into a
dispatch through an `Rc<dyn Fn>`, which is exactly the "prefer a concrete type,
carry it down to runtime" direction the north star pushes against — and the
blast radius lands on es-toolkit/remeda if either assigns a property whose name
collides with any method name in the same crate.

B is the honest floor: it is exact, needs no new fact, and covers the hono
shape, because `this.match = …` names its own class syntactically. It does not
cover assignment through an instance from outside the class.

**Recommendation: B now, C when a corpus actually needs it.** B is a strict
subset of C's behaviour — every member B converts, C also converts — so B does
not have to be unwound to get to C later. A is the only option that can regress
unrelated code, and it buys nothing hono needs.

## Verification plan (either option)

1. A runtime fixture where the reassignment changes observable behaviour: a
   class whose method is replaced mid-run, asserting the post-assignment calls
   take the new body and the pre-assignment ones took the old, plus a second
   method on the same class that is never assigned and must stay an inherent
   method (so the rule is proven to be scoped, not blanket).
2. es-toolkit and remeda byte-identical unless they contain the shape; if
   either does, the diff is inspected member by member and reported.
3. `SmeltUnknown` report on the examples corpus: avoidable stays 0. Option A
   would be the one at risk here, which is part of why it is not the
   recommendation.
4. hono slice: 2 errors cleared, and no new ones from the changed
   representation of `SmartRouter::match`.

## Not in this note

`SmartRouter::match` is *also* read as a value elsewhere
(`matcher.ts:12`'s `(this as any).buildAllMatchers()` is the neighbouring
`E0615`). That one is a different bug with a different cause — a receiver that
downstream specialization resolved to a concrete class while the read came
through an `as any` cast — and four fixtures failed to reproduce it. It is not
fixed by H43 and should not be folded into it.

---

## What landed (round 16)

Option **B**, in `smelt-frontend-ts` only — the emitter needed no change, as
predicted:

- `methods_assigned_on_this` scans a class body for `this.<name> = …`
  (static or string-literal key, `this` receiver, walking into nested closures).
  Syntactic, no type resolution, no new HIR fact.
- A non-static method whose name is in that set is lowered as a **field** whose
  type is `method_signature_function_type` (the method's own signature, so a
  call through the field keeps the declared parameter and return types) and
  whose initialiser is the method's own body, lowered through
  `function_expression_value` — the same path a `f = function () {…}` field
  initialiser already took. `ClassFieldInit` is the small enum that lets a
  field initialiser come from either an expression or a method.
- The method-lowering loop skips exactly those methods, so nothing is emitted
  twice.

Generated shape, from the new end-to-end example:

```rust
pick: ::std::rc::Rc<dyn Fn(String) -> String>,     // the reassigned method
this.0.borrow_mut().pick = _smelt_tmp_1.clone();   // its own body, in `new`
self.0.borrow_mut().pick = _smelt_tmp_4.clone();   // the reassignment
fn describe(&self, path: String) -> String { … }   // the untouched sibling
let _smelt_tmp_7: String = self.describe(path.clone());  // still a STATIC call
```

### Verification

- `examples/typescript/end-to-end/55_reassigned_method` — a runtime fixture
  where the reassignment changes behaviour and persists across calls:
  `slow:a,fast:a` / `fast:b,fast:b` / `[a][b]`. The first line proves the
  original body ran before the assignment and the new one after; the second
  proves the assignment stuck on the instance; the third proves the untouched
  sibling still works. This matches JavaScript, where the assignment creates an
  own property shadowing the prototype method.
- `a_method_reassigned_through_this_becomes_a_callable_field` asserts the
  emitted text on both halves, including three negative assertions that the
  sibling stays an inherent method with a static call and does NOT become a
  field. That scoping is the point: converting every method would trade a
  static call for an `Rc<dyn Fn>` everywhere.
- `cargo test --workspace`: 2671 passed, 0 failed. Every pre-existing golden
  is unchanged, which is the corpus-level evidence that the rule is scoped.
- es-toolkit and remeda are **byte-identical, because neither contains the
  shape**: remeda has no `this.<name> = …` in non-test sources at all, and
  es-toolkit's three (`this.name` in `server/exec.ts`, `this.capacity` and
  `this.available` in `promise/semaphore.ts`) all assign data fields, none of
  which is also a method. So the rule cannot fire there.
- SmeltUnknown examples invariant: avoidable 0, +0 versus baseline.

### Known consequences, recorded deliberately

1. A class with an assigned method becomes a reference class
   (`Rc<RefCell<…Inner>>`) if it was not one already, because the field is
   mutable. That is inherent to making the member assignable.
2. A call through the field is an indirect call, so a method that calls the
   reassigned member becomes fallible (`run` in the fixture returns
   `Result<…>`). Direct calls to the untouched siblings are unaffected. This
   is the ordinary cost of the member genuinely being a function value.

### Still open

External assignment — `instance.method = …` from outside the class — remains a
**named blocker**. The receiver's class is not knowable from the AST, so it
needs the `(class, member)` HIR fact described as option C above. Nothing in
hono's slice needs it. Every member option B converts, option C also converts,
so widening later does not have to unwind this.
