# es-toolkit blocker: `const item expression shape is not supported for inlining yet`

Investigation and fix report for the TypeScript-frontend lowering blocker that
aborted the whole-crate es-toolkit build (pinned ref `e008a2818cd8`) at
`src/predicate/isEqualWith.spec.ts` and affected 29 spec files.

## Failure family

Importing (or referencing same-module) `const` items whose initializer is a
*computed* expression failed to inline into the referencing body:

```
src/compat/_internal/arrayViews.ts   export const arrayViews = [...typedArrays, 'DataView'];
src/compat/_internal/empties.ts      export const empties = [[], {}].concat(falsey.slice(1));
src/compat/_internal/whitespace.ts   export const whitespace = [...].filter(chr => /\s/.exec(chr)).join('');
src/compat/_internal/args.ts         export const args = toArgs([1, 2, 3]);
```

Any spec importing one of these consts died with
`const item expression shape is not supported for inlining yet`.

## Root cause

`ModuleBuilder::clone_const_body_expr`
(`crates/smelt-frontend-ts/src/lowering/expr/references.rs`) copied a const
item's initializer expression tree into the referencing body through a
hand-maintained **whitelist** of ~20 `ExprKind` variants (literals, calls,
fields, container literals, conditionals, bin/unary ops). Every other
expression kind — `ListConcat` (array spread / `.concat`), `ListSlice`
(`.slice`), `ListCallback` (`.filter`), `StringJoin` (`.join`), `Method`,
`Index`, regex kinds, and roughly 140 more — fell into the `_ =>` arm and
raised the diagnostic, even though the const body had already lowered those
kinds successfully.

A secondary defect surfaced once inlining succeeded: array-spread literals
with plain elements (`[...typedArrays, 'DataView']`) blanket-typed their items
as `Unknown` (`array_spread_item_type` returned `Unknown` whenever any
non-spread element existed). The inlined spread operand kept its concrete
`List<String>` type, the surrounding concat chain was `List<Unknown>`, and the
Rust emitter silently produced an empty `SmeltList::default()` for the
mismatched concat — generated tests then failed at runtime with wrong values.

## General rules implemented (no per-library special cases)

1. **Exhaustive structural child remapping** — new
   `ExprKind::try_map_child_exprs` in `crates/smelt-hir/src/expr/map.rs`
   rebuilds any expression kind with each direct child `ExprId` passed through
   a mapping function. The match is exhaustive (no `_` arm), so a future
   `ExprKind` variant forces the mapper to be updated.
   `clone_const_body_expr` now recurses through this mapper, so **every**
   expression shape a const initializer can lower to also inlines. The only
   rejected shapes are the ones that genuinely reference the source body's
   local arenas — `ExprKind::Local` and `ExprKind::Block` — which cannot move
   across bodies by expression cloning alone (they do not occur in
   module-level const initializers es-toolkit uses).

2. **Spread-literal item type unification** —
   `array_expression_with_spread`
   (`crates/smelt-frontend-ts/src/lowering/expr/operators.rs`) now lowers all
   elements first (in source order, preserving evaluation order), then unifies
   the item type from every piece: spread operands contribute their unwrapped
   `List`/`Set` item type (or `String` for string spreads), plain elements
   contribute their expression type. A single agreed candidate wins; mixed or
   erased candidates fall back to `Unknown` exactly as before. This keeps
   `[...typedArrays, 'DataView']` a concrete `List<String>` — a net
   *reduction* in `SmeltUnknown` usage, per the SmeltUnknown boundary rules.

## Shapes fixed vs deferred

Fixed (verified end-to-end via fixture `smelt build` + generated `cargo test`):

- array spread in array literal: `[...xs, 'y']` (`ListConcat`)
- method chains: `[a, b].concat(other.slice(1))` (`ListConcat` + `ListSlice`)
- callback chains: `[...].filter(cb).join('')` (`ListCallback` + `StringJoin`
  + cloned `Closure` reference)
- every other `ExprKind` with only crate-global references now clones
  structurally (calls, indexing, regex, string/list/dict/set operations, ...)

Deferred (unchanged behavior, now with a precise diagnostic
`const item expression references body-local state that cannot be inlined`):

- const initializers whose HIR references body-local state (`Local`, `Block`).

Out-of-scope defects observed while verifying (pre-existing, reproduce without
any const imports):

- `filter(chr => /\s/.exec(chr))` generates
  `Option<SmeltMatch>.map_or(false, |value| value)` — regex-exec truthiness in
  predicate callbacks does not compile (`expected bool, found SmeltMatch`).
  This will surface when es-toolkit builds reach `whitespace`-consuming specs
  at codegen level; HIR lowering is fine.
- `importedConst.join(',')` takes the namespace-import static-join path in
  `string_join_call` because the receiver identifier is in `value_imports`,
  and mis-treats the separator as the array argument.

## Verification

- `cargo check --workspace` clean.
- `cargo clippy` on touched crates (`smelt-hir`, `smelt-frontend-ts`,
  `smelt-codegen-rust`) clean. (`--all-targets` reports pre-existing pedantic
  lints in test modules untouched by this change.)
- `cargo test --workspace --exclude smelt-gui` green.
- Regression tests added:
  - `smelt-frontend-ts::tests::part01_tests::imported_const_with_array_spread_initializer_inlines`
  - `smelt-frontend-ts::tests::part01_tests::imported_const_with_method_chain_initializer_inlines`
  - `smelt-frontend-ts::tests::part01_tests::imported_const_with_callback_chain_initializer_inlines`
  - `smelt-codegen-rust::tests::part_7_tests::inlined_spread_const_emits_concrete_string_list_concat`
  - `smelt-codegen-rust::tests::part_7_tests::inlined_method_chain_const_emits_concat_and_slice`
- Fixture project (spread + concat/slice + filter/join consts imported across
  modules): `smelt build` succeeds and the generated crate's `cargo test`
  passes 3/3.

### es-toolkit re-check (all 29 previously-affected files, single-file dump-hir)

The `const item expression shape is not supported for inlining yet` family is
gone from **all 29 files**. 16 now lower cleanly:

find, flatMap, findLast, pullAt, reduceRight, reduce (compat/array); mean
(compat/math); at, get, result (compat/object); trim, trimEnd, trimStart
(compat/string); toFinite, toInteger (compat/util).

13 keep *unrelated pre-existing* diagnostics:

- `array callback callback item parameter count is not supported` (10):
  every, includes, sample, some (compat/array); sum, sumBy (compat/math);
  keys, keysIn (compat/object); isArguments, isEmpty (compat/predicate);
  constant (compat/util)
- `dynamic computed access on the global object requires the runtime global
  object (not yet modeled)` (2): compat/predicate/isEqual,
  predicate/isEqualWith — from `globalThis[type]`
- `callback item references must resolve to callable values` (1):
  compat/util/toNumber

### Whole-crate build abort point

- Before: `src/predicate/isEqualWith.spec.ts` —
  `const item expression shape is not supported for inlining yet`.
- After: still `src/predicate/isEqualWith.spec.ts`, but the abort is now the
  *next* family in the same file: `dynamic computed access on the global
  object requires the runtime global object (not yet modeled)`
  (`globalThis[type]` inside the arrayViews loop). The const-item family no
  longer aborts anything.
- With the two `globalThis` specs (`predicate/isEqualWith`,
  `compat/predicate/isEqual`) additionally excluded in a scratch copy, the
  build reaches `src/predicate/isFunction.spec.ts`
  (`unresolved identifier \`Proxy\``) — matching the "next blockers" already
  documented in `.github/compat/es-toolkit/Smelt.toml`.
