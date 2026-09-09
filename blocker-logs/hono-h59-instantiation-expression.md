# H59 — a value-position instantiation expression is not lowered

Found while attempting H44's crate-level reproduction (round 25). **Not fixed**,
and small.

## What happens

```ts
export function match<R extends Router<T>, T>(this: R, method: string, path: string): T[] { … }

class RegExpRouter<T> implements Router<T> {
  match = match<RegExpRouter<T>, T>;   // <- instantiation EXPRESSION
}
```

```
expression kind is not lowered yet: TSInstantiationExpression(…)
```

A TypeScript 4.7 instantiation expression pins a generic function's type
arguments without calling it. It is a type-level annotation on a value: the
value is the same function, so lowering it is lowering the inner expression and
discarding the arguments — or, better, applying them as the hint the equivalent
`const f: typeof match<A, B> = match` would give.

## Where it appears

Hono itself writes the type-position form,
`match: typeof match<Router<T>, T> = match` (`reg-exp-router/router.ts:129`),
which DOES lower today. The value-position form is what a hand-written
reproduction reaches for first, and es-toolkit/remeda have neither, so this is
not currently blocking any corpus — it is recorded because the diagnostic names
it precisely and the fix is a few lines in the expression dispatch (unwrap to
`expression`, keep the type arguments as a hint).

## Note on scope

Discarding the type arguments outright would lower it as if it were the bare
function, which is right for the VALUE but throws away the instantiation the
source asked for; where the arguments matter, they matter for the same reason
H41's turbofish work did. Worth doing with the hint rather than as a bare
unwrap.
