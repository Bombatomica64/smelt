# Remeda Runtime Failure Plan

## Goal

Remeda should compile and run with JavaScript-compatible runtime behavior instead of accumulating
module-specific fixes. The current generated crate compiles, and focused `difference` tests pass,
but the full Remeda test suite still exposes several broad semantic gaps.

This plan groups those failures by root cause and defines implementation slices that should fix many
tests at once.

## Failure Families

### 1. Object Graph Clone, Equality, And Hashing

Smelt now preserves object identity for `Map`-like key comparisons, but structural operations still
need JavaScript object graph semantics.

Symptoms:

- `clone` tests involving nested objects and circular references.
- `isDeepEqual` and `isShallowEqual` object tests.
- `constant` and `defaultTo` identity preservation tests.
- Full Remeda suite aborting with stack overflow on circular object comparisons.

Required behavior:

- `Map` and `Set` key comparisons use SameValueZero:
  - primitives compare by value;
  - `NaN` equals `NaN`;
  - objects and functions compare by identity.
- `toStrictEqual` and Remeda deep equality use structural equality:
  - object properties compare recursively;
  - repeated/circular references do not recurse forever;
  - two references already compared as equal during the same traversal remain equal.
- Runtime hashing must not recursively traverse cyclic object graphs.
  - Prefer identity hashing for key containers.
  - If structural hashing is still needed, it must be cycle-aware.
- `clone` must distinguish shallow identity-preserving clones from deep structured clones.

Implementation direction:

- Add generated runtime helpers for `SmeltUnknown` structural equality with a visited object-pair
  set.
- Make `PartialEq for SmeltUnknown` call the structural helper.
- Make `SmeltObject` structural equality call the same helper instead of comparing borrowed maps
  directly.
- Remove or restrict recursive structural `Hash` for `SmeltUnknown::Object`; object-key maps should
  use `SmeltJsMap`, and any remaining `Hash` path must not stack overflow.
- Add focused tests for:
  - two equivalent cyclic objects;
  - two different cyclic objects;
  - repeated shared references;
  - Map key identity still differing from structural equality.

### 2. Callable Object And Lazy Pipeline ABI

Remeda uses callable objects and lazy pipeline metadata heavily. Treating callable objects as
ordinary unknown maps with scattered special cases causes state loss and inconsistent lazy behavior.

Symptoms:

- `pipe`, `filter`, `find`, `flatMap`, `dropWhile`, `dropFirstBy`.
- `intersection`, `differenceWith`, `intersectionWith`.
- Data-last lazy tests.

Required behavior:

- A callable object is an object with callable state and normal properties.
- Reading the call slot must not mutate the object.
- Cloning a callable object must preserve callable identity and property identity according to JS
  object reference rules.
- Lazy result records (`done`, `hasNext`, `next`, `hasMany`) must preserve shape and callable state
  across erased `unknown` boundaries.

Implementation direction:

- Centralize callable extraction in one generated runtime helper, for example
  `smelt_callable_from_unknown`.
- Replace all generated `__smelt_call` pattern snippets with that helper.
- Represent callable objects directly in the runtime model if helper-based lowering remains too
  fragile.
- Add tests for repeated callable-object calls after clone, lazy data-last invocation, and lazy
  result record field reads.

### 3. Catchable Exceptions And `toThrow`

Several tests check invalid argument handling with `expect(...).toThrow(...)`. These should use the
same exception representation as source `try/catch`, not panic paths hidden inside stdlib lowering.

Symptoms:

- `ceil`, `floor`, `chunk`, `conditional`, and other boundary validation tests.

Required behavior:

- Operations that throw in TypeScript lower to `Result`/catchable exceptions.
- `expect(fn).toThrow(...)` catches those exceptions.
- Uncaught exceptions still crash the generated test/function normally.
- Panics are reserved for internal Smelt invariants, not source-level exceptions.

Implementation direction:

- Audit stdlib operations that currently call `panic!` for source-level errors.
- Convert source-level throws to generated `Err(...)`.
- Ensure callback adapters propagate `Result` instead of converting errors to panics when the source
  exception is catchable.

### 4. JavaScript Object Property Semantics

Object key conversion and own-property iteration are still split across emitters.

Symptoms:

- `entries`, `fromEntries`, `fromKeys`, `groupBy`, `groupByProp`, `invert`, `hasProp`,
  `forEachObj`.

Required behavior:

- Number keys stringify for plain object properties.
- Symbol keys are preserved or skipped according to the JS API being lowered.
- Own-property checks distinguish missing keys from present keys whose value is `undefined`.
- Object entry/value/key projection uses one central own-property implementation.

Implementation direction:

- Centralize property key conversion and own-property projection in generated runtime helpers.
- Route dict projections, `Object.*`, and unknown property checks through those helpers.
- Add tests for number keys, symbol keys, missing vs present undefined, and prototype-like names.

### 5. Optional, Default, And Rest Call ABI

Some data-last and funnel/debounce/throttle failures come from fallback argument synthesis instead
of general TypeScript call ABI lowering.

Symptoms:

- `debounce`, `throttle`, `funnel`, and functions with optional options records.
- Data-last calls where missing arguments become the wrong empty container.

Required behavior:

- Missing optional arguments lower through the target parameter type's default representation.
- Empty object options lower to the same object/record representation as non-empty object literals.
- Rest parameters preserve rest metadata and do not conflate normal array parameters with rest tails.

Implementation direction:

- Remove function-name-sensitive fallback argument rewrites.
- Use target parameter type information to synthesize missing arguments.
- Add call-boundary tests for optional records, empty object literals, rest callbacks, and data-last
  wrappers.

## Work Order

1. Implement cycle-aware object graph equality and non-recursive object hash safety.
2. Centralize callable object extraction and lazy ABI handling.
3. Convert stdlib source-level errors to catchable exceptions.
4. Centralize JS object property semantics.
5. Replace remaining optional/default/rest argument fallback hacks with ABI-driven lowering.

## Acceptance Criteria

- Full generated Remeda crate still compiles.
- Focused `difference` remains 13/13 passing.
- Circular object equality/clone tests no longer abort the full suite.
- Each fixed failure family has focused codegen/runtime tests in Smelt, not only Remeda evidence.
- No new function-name-specific special cases are introduced.
