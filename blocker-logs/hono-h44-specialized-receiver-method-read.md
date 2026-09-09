# H44 — a dynamic member read on a specialized receiver treats a method as a field

Round 16, item 2. **1 slice error**, numbered and left per the ruling. Not
fixed: the precondition cannot be reached from a small fixture.

## The error

```
src/main.rs:7133: error[E0615]: attempted to take value of method
  `build_all_matchers` on type `&__smelt_anon_class_3050<T>`: method, not a field
```

The generated read:

```rust
let smelt_function_value = self.build_all_matchers.clone();
// … then called through the SmeltUnknown::Function dispatch
```

`__smelt_anon_class_3050` really does have `build_all_matchers` as an inherent
method (`impl<T: …> __smelt_anon_class_3050<T> { … fn build_all_matchers(&self) … }`),
so the read is asking for a field that is a method.

## The source

`third_party/hono/src/router/reg-exp-router/matcher.ts:10`:

```ts
export function match<R extends Router<T>, T>(this: R, method: string, path: string): Result<T> {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const matchers: MatcherMap<T> = (this as any).buildAllMatchers()
```

A free function with an explicit `this: R` parameter bounded by `Router<T>`,
reading a member through an `as any` cast. `buildAllMatchers` is `protected` on
the concrete routers and is not on the `Router` interface at all, which is why
the source needs the cast.

## The precondition, and why no fixture reaches it

The read has to happen on a receiver that **downstream specialization resolved
to a concrete class** while the read itself came through the erased (`as any`)
path. Where the receiver stays erased, the emitter takes the correct dynamic
route and everything works.

Four fixtures were written and all four emit the CORRECT route:

| fixture | emitted |
| --- | --- |
| `const f = this.build; f()` on a named class | bound-method closure, `SmeltUnknown::Function(smelt_method)` |
| `(this as any).build()` on a named class | `smelt_get_unknown_field(&_smelt_tmp_1, "build")` |
| `function run<R extends Base>(this: R)` with `Base` an interface | `smelt_bind_this(smelt_get_unknown_field(…, "build"), …)` |
| the same with `Base` a class and a `protected` method | `smelt_bind_this(smelt_get_unknown_field(…, "build"), …)` |

So the bug is not "a method read as a value" — that path is right. It needs the
specialization decision, which is whole-crate: no small fixture emits the
callee at a concrete instantiation, exactly the shape of problem H41a hit.

## What a fix would look like

For a receiver whose static type resolved to a concrete class, a member read of
a name the class declares as a METHOD must produce a bound-method closure (or a
direct call), never a struct-field access. The correct route already exists for
the erased receiver; the specialized-receiver path needs to reach it.

Verifying it needs either a specialization-level fixture (a test that pins the
instantiation rather than relying on a corpus) or the hono slice itself as the
witness. That choice is worth making deliberately rather than as a side effect
of a one-line fix, which is why this is numbered and left.

## Relation to H43

None, beyond both being `E0615` in the same file. H43 was a method that is
ASSIGNED, and is fixed by carrying it as a callable field. This is a method that
is READ on a specialized receiver, and carrying it as a field would be wrong —
it is never assigned, and a field would trade a static call for a dynamic one.
The two must not be folded together.

## Round 25: a fifth attempt, at crate level, and it still does not reproduce

The round-25 ruling was "via a specializer unit fixture if the precondition can
be reproduced there; otherwise leave numbered". The four fixtures above were
single-module; this attempt was a whole three-module crate, built and
`cargo check`ed, because the note says the missing ingredient is a
specialization decision and that decision is whole-crate:

```
src/router.ts   interface Router<T> + the `this: R extends Router<T>` free
                function reading `(this as any).buildAllMatchers()`
src/regexp.ts   class RegExpRouter<T> implements Router<T>, with a PROTECTED
                `buildAllMatchers(): T[]` and `match: (…) => T[] = match`
src/main.ts     instantiates `RegExpRouter<string>` and calls `router.match(…)`
```

That is Hono's arrangement, module for module. The generated crate fails, but
with a DIFFERENT error:

```
error[E0308]: mismatched types: expected `SmeltList<T>`, found `SmeltList<SmeltUnknown>`
```

which is the H42 `TypeParam` erasure pair, not `E0615`. So the read still takes
the correct dynamic route here; the precondition needs something the slice has
and this crate does not, and one more guess is not worth another round.

**H44 therefore stays numbered**, now with the crate-level attempt recorded so
it is not repeated. The witness remains the slice itself, which is the honest
place to verify a whole-crate specialization decision.

One thing the attempt did find on the way: Hono spells the wiring
`match: typeof match<Router<T>, T> = match` (router.ts:129), and a
value-position instantiation expression — `match<RegExpRouter<T>, T>` — is
"expression kind is not lowered yet: TSInstantiationExpression". Recorded
separately as `blocker-logs/hono-h59-instantiation-expression.md`.
