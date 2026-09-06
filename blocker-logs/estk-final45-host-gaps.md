# es-toolkit: the two rows that need a host capability

After the final-45 campaign (`blocker-logs/estk-final45-plan.md`) es-toolkit stands at
1055 / 4. Two of the four remaining rows are type-system work (`at`, `mergeWith`; see the plan).
These are the other two. Neither is a lowering defect: each assertion is only true when a
specific host runtime is present, and Smelt's profile deliberately does not provide it.

## 1. `isBrowser should return true in browser environment`

### Source shape

`src/predicate/isBrowser.ts`:

```ts
declare let window: { document: unknown } | undefined;

export function isBrowser(): boolean {
  return typeof window !== 'undefined' && window?.document != null;
}
```

`src/predicate/isBrowser.spec.ts`:

```ts
// @vitest-environment happy-dom
it('should return true in browser environment', () => {
  expect(isBrowser()).toBe(true);
});
```

### What Smelt emits

```rust
pub(crate) fn is_browser() -> bool {
    return false;
}
```

### Why it needs a host capability

The function's whole job is to ask the host whether a DOM `window` exists. The `declare let`
creates no binding; it asserts the host already provides one. Smelt's profile is non-DOM by
construction (`global_member_presence("window") == Absent`), so the `typeof` guard folds to
`false` at compile time and the emitted body is the honest answer for every environment the
generated crate can run in.

The spec passes only because vitest swaps in `happy-dom`, a JavaScript DOM implementation that
defines `window` and `document` for the duration of the file. Making this row pass would mean
either shipping a DOM emulation in the runtime prelude or letting a spec-file pragma flip the
compile-time profile. Every other `@vitest-environment happy-dom` spec in es-toolkit is already
in the corpus `exclude` list in `.github/compat/es-toolkit/Smelt.toml`; this one was overlooked
and belongs there too.

## 2. `isPlainObject should return true for cross-realm plain objects`

### Source shape

`src/predicate/isPlainObject.spec.ts`:

```ts
import { runInNewContext } from 'node:vm';

it('should return true for cross-realm plain objects', async () => {
  expect(isPlainObject(runInNewContext('({})'))).toBe(true);
});
```

`src/predicate/isPlainObject.ts` (the clause the test exercises):

```ts
const proto = Object.getPrototypeOf(value);
const hasObjectPrototype =
  proto === null ||
  proto === Object.prototype ||
  // Required to support node:vm.runInNewContext({})
  Object.getPrototypeOf(proto) === null;
```

### What Smelt emits

The unresolved `node:vm` import lowers to an empty record, so `runInNewContext` is a call of a
non-callable and yields `null`; `isPlainObject(null)` is correctly `false`, and the assertion
fails.

### Why it needs a host capability

`runInNewContext('({})')` does two things Smelt cannot: it **evaluates JavaScript source text at
runtime**, and it does so **in a second realm**, whose `Object.prototype` is a different object
from the caller's. The library clause exists precisely for that second realm: the returned
object's prototype is a foreign `Object.prototype`, so the test is checking that `isPlainObject`
walks one more level and finds `null`.

In generated Rust there is exactly one `Object.prototype` (the `__smelt_proto:object` sentinel)
and no interpreter. This is NOT a concurrency question: `vm` contexts run synchronously on the
caller's thread and heap, so a fork or green thread would isolate the wrong thing. It is now
scoped as deferred family D3 in `estk-final45-plan.md`: a constant source string handed to an
evaluator is a compile-time program, and a realm is a tag on the intrinsic sentinels. Only a
runtime source string genuinely needs an embedded engine.
The other eleven `isPlainObject` cases pass, including the same-realm `Object.create({})` chain
walk that Batch T fixed, so the predicate itself is fully modeled.

## One hygiene point worth doing anyway

Both rows fail *quietly*: an absent host global folds to a constant and an unresolved module
import becomes an empty record whose call answers `null`. That is why they were counted as
defects for several passes. An unresolved import (`node:vm`) should be a named blocker rather
than a fabricated value, so a spec that needs an unmodeled host shows up as "unsupported" in the
report instead of as a false assertion. That is a small frontend change (`S`) and does not make
either test pass.
