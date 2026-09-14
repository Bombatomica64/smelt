# H58 — a callback whose body is a stdlib method call types as `unknown`

Found while writing H54's fixture (round 25). **Not fixed**, pre-existing, and
independent of H54: the shape is an EXPRESSION-bodied callback, which already
inferred its own return type.

## Repro

```ts
const a = ["a", "b"].map((key) => key.toUpperCase());   // List<Unknown>
const b = ["a", "b"].map((key) => key + "!");           // List<String>
const c: string[] = ["a", "b"].map((key) => key.toUpperCase()); // List<String>
```

The three differ only in the callback body and the annotation. HIR:

```
a:  s0: let %0: List<Unknown> = #4
b:  s0: let %0: List<String>  = #4
c:  s0: let %0: List<String>  = #4
```

So the mapped element type is lost when the callback's whole body is a stdlib
METHOD CALL on the parameter, and an explicit annotation on the binding recovers
it. `key + "!"` — an operator whose result type the frontend computes — does
not lose it.

## Why it matters

`xs.map((x) => x.trim())`, `.toUpperCase()`, `.slice()`, `.toString()` are
everyday JavaScript, and the cost is silent: the list is `List<Unknown>`, so
every element is erased and re-tagged, and the erasure only surfaces later where
something needs the concrete type (H54 surfaced exactly that way, as
`list unshift item must match the list element type` two frames from the map).
Any avoidable-erasure count in the corpora carries some number of these.

## Where to look

The compact callback IR types a `CallbackExprKind::Call` from the callee's
return type; a stdlib method call is not an item call, so the arm that models it
(`callbacks/dispatch.rs`'s member-call handling, and
`callback_stdlib_member_call` if that is the path) is where the result type
either is not consulted or is not available. The same stdlib member resolution
used outside callbacks (`string_affix_call`, `string_case_call`, ... in
`lowering/stdlib/`) answers a concrete `String` for `toUpperCase`, so the type
exists — the callback path is not asking for it.

Two candidate rules, in preference order:

1. Type the compact callback's stdlib member call from the same resolution the
   non-callback path uses, so the callback's return type is concrete.
2. Failing that, let the callback fall back to closure-body lowering, where the
   H54 inference now reads the body's own type.

(1) is the real fix; (2) would only widen the reach of a rule that already
exists.
