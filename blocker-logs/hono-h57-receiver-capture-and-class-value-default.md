# H57 (E0424) and the generic class value's default (E0283)

Round 26, item 3. Both **fixed**; the router slice goes 10 → 7 `cargo check`
errors, and every remaining one is H42 (6) or H44 (1).

## H57 — a captured receiver cannot be bound as `self`

```
error[E0424]: expected unit struct, unit variant or constant, found local variable `self`
  --> src/main.rs:22344
   |  let self = self.clone();
```

A closure that captures the enclosing method's receiver had its capture bound
under the receiver's own emitted name, which is `self`. That name made the
closure BODY work unchanged — every `self.0.borrow().field` in it reads the
capture — and Rust rejects it: `self` is a keyword and cannot be a `let`
binding.

The fix is the machinery that was already there for shared captures: the
capture takes a generated name (`smelt_self`) and the body's uses are rewritten
to it through `replace_shared_capture_uses`, which is a text rewrite that
already understands identifier boundaries, string literals and shadowing
closures. Both capture preludes in `emitter/closures.rs` do it — the sync
closure and the nested-body one — and the name lives in one `SELF_CAPTURE_NAME`
constant so prelude and rewrite cannot disagree.

Hono's `trie-router/node.ts` hit it twice: `#getHandlerSets`'s callbacks read
`this.#params` while being passed as callback values.

## The generic class value's default — E0283

```
error[E0283]: type annotations needed for `__smelt_anon_class_3050<_>`
  --> src/prepared_router.rs:121
   |  let reg_exp_router_with_matcher_export: __smelt_anon_class_3050<_> = Default::default();
```

Source (`reg-exp-router/prepared-router.ts:98`):

```ts
const RegExpRouterWithMatcherExport = class<T> extends RegExpRouter<T> { … }
```

A generic class referenced without resolved type arguments renders its slots as
inference placeholders (`Class<_>`), which is right when an initializer pins
them — `new ImmutableCache<T>()` does. A class VALUE has no such initializer:
its default is a bare `Default::default()`, so nothing pins `T` and Rust cannot
choose.

`default_value` now NAMES the class when its arguments are unresolved,
`Class::<SmeltUnknown, …>::default()`, one argument per declared parameter. A
class value is the constructor, never an instance, and every read of it goes
through erasure, so the carrier is the honest choice for a parameter the crate
never resolves. `class_type_param_count` answers the arity for both halves so
the annotation and the default cannot disagree.

## Regression guard

`examples/typescript/end-to-end/81_receiver_capture_in_callback`, verified to
fail with the closure change reverted.

The class-value half has **no fixture**, deliberately: writing one measured
that a generic class expression bound to a const is broken at RUNTIME for
independent reasons (below), so a fixture would have asserted wrong values. Its
witness is the slice's own `cargo check`.

## Found on the way: a generic class VALUE answers wrong at runtime

```ts
const Boxed = class<T> { items: T[] = []; push(item: T): number { … } };
const numbers = new Boxed<number>();
console.log(String(numbers.push(7)) + ' ' + String(numbers.items[0]));  // "null undefined"
console.log(typeof Boxed);                                              // "object"
```

JavaScript answers `1 7` and `function`. Two wrong values in one shape: the
construction through a const-bound generic class expression produces no usable
instance, and `typeof` on a class value says `object` where the language says
`function`. Numbered as **H68**, not fixed here.
