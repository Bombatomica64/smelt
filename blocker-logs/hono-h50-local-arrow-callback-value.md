# H50 — an arrow bound to a const answers WRONG when passed as a callback

Found while landing Hono blocker 3 (round 23); **ruled and FIXED in round 24**.
It was a silent wrong answer, not a blocker, which is what made it the most
serious finding of that round even though nothing reported it.

## Repro

```ts
type Predicate = (value: string) => boolean;
const annotated: Predicate = (value: string): boolean => value.length < 3;
const bare = (value: string): boolean => value.length < 3;
console.log(annotated('ab'));            // true   — correct
console.log(bare('ab'));                 // true   — correct
const words = ['a', 'abc'];
console.log(words.filter(annotated).join(','));  // prints ""  — WRONG, expect "a"
console.log(words.filter(bare).join(','));       // prints ""  — WRONG, expect "a"
```

Both spellings are wrong, with and without the type annotation, and with or
without a type assertion at the call — so it is not the assertion-transparency
rule landed in the same round, and it is not the annotation either. Verified
against the parent commit's `callbacks/dispatch.rs` as well: pre-existing.

## What the emitter produces

```rust
let mut is_short: Rc<dyn Fn(String) -> bool> =
    { let smelt_default_callback: Rc<dyn Fn(String) -> bool> =
        Rc::new(move |arg0: String| -> bool { false }); smelt_default_callback };
```

The local is initialized to the DEFAULT callback (`|_| false`) and the real
arrow is never assigned to it. A direct call `annotated('ab')` is correct
because that path inlines the callback body from `scope.callback`, bypassing the
local. The array method instead builds a closure that CAPTURES the local:

```rust
move |closure_arg_0: String, ...| { (is_short)(closure_arg_0.clone()) }
```

and calls the placeholder. Hence `false` for every element: `filter` keeps
nothing, and a `map` would produce all-default values.

## Why it matters more than it looks

`false` for every element is a plausible-looking empty result. Nothing throws,
nothing fails to compile, and the generated crate's tests would only catch it
where a test asserts a non-empty result. es-toolkit and remeda pass their suites
today, so either they do not use this shape or their uses are covered by the
inlining path — worth checking as part of the fix rather than assumed.

## Where to look

The inlined-local-callback model (`scope.callback`, `callback_expr_to_closure`,
and the `Stmt::Let` that declares a callback-typed local in
`lowering/stmt/assignments.rs`) is the area. Two candidate rules, in preference
order:

1. Assign the real closure to the local — i.e. lower the arrow initializer as a
   closure VALUE as well as registering it as an inlinable callback. The
   placeholder exists so the local has a value when every use is inlined; the
   moment a use captures it, the placeholder is wrong.
2. Never capture the local: when a callback argument names an inlinable local
   callback, inline the body into the synthesized closure instead of capturing.

(1) is the smaller and more honest change: a source-level function value should
be a real function value, and a hand-written Rust port would simply bind the
closure.

## Fixture note

`69_asserted_callback_name` deliberately covers only named ITEMS and arrow
literals for this reason; adding the local-predicate line would have put the
wrong answer into the golden corpus. The comment in that fixture points here.

## What landed (round 24)

Option (2) of the two above, and the reason is that the codebase had already
made the same decision one layer over: `identifier_expression` reads a callback
binding ONLY when it holds a value (`materialized`, or registered by another
body) and otherwise rebuilds the closure from the callback's own body. Its
comment spells out why. `named_callback_reference` — the array-callback path —
captured the local unconditionally instead, so the two disagreed about the same
binding.

The fix is four lines of condition in
`crates/smelt-frontend-ts/src/lowering/callbacks/classify.rs`: a same-body
callback whose binding was never materialized returns `None` from
`named_callback_reference`, which routes the argument through the caller's
fallback to `identifier_expression` — the path that already knows the rule.
Nothing new was invented, and a materialized binding, or one from another body,
is captured exactly as before.

Verified on nine observers, every one of which now agrees with the direct call:
`filter` (annotated and unannotated predicate), `some`, `every`, `find`,
`findIndex`, `map`, use inside another callback, and passing the predicate to an
ordinary function. The regression test is
`build_runs_named_local_callback_passed_by_value`, and it fails on the parent
commit — checked by stashing the fix and running it, not assumed.

It is a build-and-run test rather than an `examples/` fixture because
referencing a module-level arrow as a value lifts it to a named function whose
Rust name embeds the absolute source path, so `expected.rs` would not be stable
across build directories. That is its own defect, now
`blocker-logs/hono-h55-path-mangled-lifted-name.md`.
