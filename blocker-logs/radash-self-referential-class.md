# Radash regression: a self-referential class emits no fields

Found: 2026-09-06, Hono round 2 gate run. **Not caused by this stream's
changes** (attribution below), reported so it is not lost.

## Symptom

`cargo test --manifest-path third_party/radash/dist-smelt/Cargo.toml` fails to
compile the generated crate:

```
error[E0061]: this function takes 0 arguments but 1 argument was supplied
error[E0609]: no field `self_` on type `Person`
error[E0609]: no field `friends` on type `Person`
error: could not compile `radash_probe` (bin "radash_probe" test) due to 3 previous errors
```

The radash regression gate was green at 84/84 earlier in this campaign, so this
is new since then.

## Source shape

`third_party/radash/src/tests/typed.test.ts` (the `isEqual` suite):

```ts
class Person {
  name: string
  friends: Person[] = []
  self?: Person
  constructor(name: string) {
    this.name = name
  }
}
const jake = new Person('jake')
jake.self = jake
jake.friends = [jake, jake]
```

A **self-referential class**: `friends: Person[]` and `self?: Person` both
mention `Person` inside `Person`. All three errors point the same way — the
emitted `Person` struct has neither `friends` nor `self_`, and its constructor
takes no arguments, so `name` is missing too. The class's field set came out
empty, and the constructor signature came out empty with it.

`src/tests/object.test.ts` has the same shape one level looser (a `type` alias
with `friends?: Person[]`), which is worth checking once the class case is
understood — a fix for one may or may not cover the other.

## Why it is not this stream's

The Hono round-2 changes are: dependency-closure pruning for `[sources]
exclude`, the import classification that goes with it, the absent-global throw,
three names added to `NON_DOM_ABSENT_GLOBALS`, and a corrected code comment.

1. **Closure pruning is provably inert here.** `.github/compat/radash/Smelt.toml`
   has **no `exclude` key at all**, and `DependencyCollector::excluded_target`
   returns `false` immediately when the glob list is empty. No edge is filtered
   and no specifier is recorded.
2. **The import classification therefore never fires**, because the
   `excluded_modules` list it consults is empty for every radash file.
3. **The absent-global hook cannot fire.** It runs only at the
   unresolved-identifier fallthrough, and only for the six names in
   `NON_DOM_ABSENT_GLOBALS`. Radash contains no bare *reference* to any of
   them: the only matches in `src/` are the `self?: Person` field
   **declaration** above and two arrow **parameters** named `self` in
   `src/curry.ts`, none of which reaches identifier resolution as a global.
4. **The failing path is untouched.** Class field and constructor emission is
   not in the round-2 diff; the only codegen change in it is a doc comment in
   `emitter/strings.rs`.

## Most likely origin

The shared branch merged six standards-stream commits into this worktree at
`ddfd6c3`, including "Milestone 0: stop the express false greens, and the `??`
type join" and the `Headers` / `URLSearchParams` runtime-type work. A change to
how a type alias or optional type is joined is the kind of change that would
drop a recursive class's field set, and that commit is the one in range that
touches type joining.

This is a hypothesis, not a finding: confirming it means building `smelt` at
`ddfd6c3` and at its parent and transpiling radash with each. That was started
and abandoned on wall-clock grounds, so **it is not verified**. What is verified
is the four points above.

## Suggested next step

Whoever owns the `??` type join should transpile radash at their commit's parent
and at their commit. A minimal repro to add to the frontend suite either way:

```ts
class Node2 {
  children: Node2[] = []
  parent?: Node2
  constructor(public label: string) {}
}
```

The assertion is that the emitted struct has all three fields and a
one-argument constructor. There is no such test today, which is why a
self-referential class could regress silently.

## Resolution (orchestrator, 2026-09-06)

Does not reproduce. The radash gate was re-run from a clean `dist-smelt` at both the
merged head `d92843f` and the Hono round-2 head `041fe87`, each with a freshly built
`smelt`: the generated crate compiles, `Person` carries `name`, `friends` and `self_`, and
`cargo test` reports 84 passed / 0 failed both times. The failure recorded above came from a
local artifact of the run that observed it (the disk reached 100% during that round, and the
generated crate was not regenerated from clean). The `??` type-join hypothesis is withdrawn.
The minimal repro above is still worth a fixture, since no test pins this shape today.
