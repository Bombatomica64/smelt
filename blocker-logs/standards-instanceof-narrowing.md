# `x instanceof <concrete host class>` does not narrow, and the fallback answers a wrong value

**STATUS: part (1) FIXED (round 10), re-verified round 16.** All four spellings
below now agree with Node 22, and a six-shape narrowing sweep agrees too. Part
(2) — the erased method-read fallback — is still open, and round 16 found it
cannot be a STATIC blocker as this note proposes: at the emit site the receiver
carries no marker statically. See
`blocker-logs/standards-narrowing-sweep-and-nullish-stringify.md` for the
re-run, the two implementable shapes, and an unrelated systemic find alongside
it.

Found while scoping the `Blob`/`File` upgrade (standards item 2), 2026-09-07.
Not fixed in that round; recorded here with the repro and the fix shape.

## The repro

```ts
function isHeaders(x: unknown): x is Headers {
  return x instanceof Headers;
}

const value: unknown = new Headers({ "content-type": "text/plain" });
if (isHeaders(value)) {
  console.log(value.get("content-type") ?? "null");   // Node: text/plain   Smelt: text/plain
}

const direct: unknown = new Headers({ a: "b" });
if (direct instanceof Headers) {
  console.log(direct.get("a") ?? "null");             // Node: b           Smelt: null
}
```

Both branches are the same narrowing in TypeScript. The first one is right and
the second one is silently wrong.

## Why

`instanceof_local_guard` (`lowering/testing/matchers.rs`) narrows through
`filtered_union_members`, which returns `None` for anything that is not a
`Type::Union`. An `unknown` local therefore keeps its erased type across the
guard, and the member read falls through to the erased member path:

```rust
_smelt_tmp_11 = matches!(direct.clone(), SmeltUnknown::Object(value) if value.contains_key("__smelt_headers"));
if _smelt_tmp_11 {
    _smelt_tmp_12 = smelt_get_unknown_field(&direct.clone(), "get").clone();
    // ... no `get` on the erased record, so the adapter falls back to
    // a default callback that answers SmeltUnknown::Null
```

Two separate defects stack up here:

1. **The guard does not narrow.** A user type predicate (`x is Headers`) DOES,
   and it emits exactly the right thing —
   `<SmeltHeaders as SmeltFromUnknown>::smelt_from_unknown(value)`. So the
   machinery to materialize the narrowing already exists and is already used;
   only the inline `instanceof` form fails to ask for it.
2. **A method read on an erased modeled record answers `undefined` instead of
   blocking.** The `__smelt_headers` record carries the header pairs but no
   callable members, and the erased-call adapter substitutes a default callback
   returning `SmeltUnknown::Null` rather than reporting that the member is not
   modeled. That turns every missed narrowing into a wrong value instead of a
   named blocker, which is what hid defect 1.

## Fix shape

For (1): in `instanceof_local_guard`, when the local's type is not a union,
narrow it to the target class whenever the target resolves to a class whose
values have a concrete Rust representation reachable from an erased value —
i.e. the class declares a checked `SmeltFromUnknown` adapter. That set is
`Headers`, `URLSearchParams`, `Response`, `Request`, `RegExp`, the synthetic
match classes, and (after standards item 1) the concrete byte view. Scoping the
rule to "there is a sound checked cast for this class" is what keeps it a
general rule rather than a list of spellings: the narrowing is emitted exactly
where it can be materialized.

Expect this to move generated code in es-toolkit, whose `cloneDeepWithImpl`
does `valueToClone instanceof RegExp` and then reads `.source`/`.flags`/
`.lastIndex` off it — those reads become concrete `SmeltRegExp` reads instead of
erased field reads. That is almost certainly more correct, but it is a codegen
change across the corpus and needs the full ratchet plus the generated test
suite, which is why it was not folded into a standards item.

For (2): the erased method-read fallback should be a named blocker for a
receiver that carries a modeled host marker. A record that Smelt built and
knows the shape of is exactly the case where "member not modeled" is
knowable, and answering `undefined` there is never right.
