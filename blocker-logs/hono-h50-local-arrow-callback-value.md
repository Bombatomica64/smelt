# H50 — an arrow bound to a const answers WRONG when passed as a callback

Found while landing Hono blocker 3 (round 23). **Not fixed.** This is a silent
wrong answer, not a blocker, which makes it the most serious thing in this
round's findings even though nothing reported it.

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
