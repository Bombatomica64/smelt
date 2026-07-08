# Optional-chained method calls on modeled stdlib receivers

## Summary

`recv?.method(args)` where `recv: T | undefined` (or `T | null`) and `T` is a
modeled stdlib receiver (`Map`, `Set`, ...) previously failed to lower: the
optional-chained collection method fell through the stdlib dispatch to the
generic static-member path, which mis-typed `stack?.has(x)` as an erased field
read (`stack.get("has")`) returning `SmeltUnknown`. This blocked es-toolkit
`src/compat/predicate/isMatchWith.ts` (`stack?.has` / `stack?.set` /
`stack?.delete` on a `Map<any, any> | undefined`) and every spec importing it.

The fix desugars the optional-chained modeled-receiver call generally, in the
same stdlib dispatch that already handles the non-optional receiver:

```
recv?.method(args)   ==>   recv-present ? Some(<modeled op on narrowed recv>)
                                        : undefined
```

so the result type is `R | undefined` through the existing optional machinery,
and the operation itself is the *same* modeled HIR node the non-optional
receiver produces (`DictContainsKey`, `DictGet`, `DictSet`, `DictRemoveKey`,
`SetContains`, `SetAdd`, `SetRemove`, projections). No `SmeltUnknown` is
introduced.

## Implementation

`crates/smelt-frontend-ts/src/lowering/stdlib/collections.rs`

* `dispatch_collection_method` now recognizes an `Optional(inner)` receiver
  whose inner type is a modeled `Map`/`Set`. It lowers the receiver once, builds
  the modeled operation on a **narrowed** receiver, and wraps the result.
* `narrowed_optional_receiver` builds a `TypeAssert` typed as the inner receiver
  type. The Rust emitter already unwraps an `Optional(inner)` operand assigned
  into an `inner` slot (`.clone().expect("optional value was absent after
  narrowing")`) — the exact narrowing the ordinary `if (recv) { recv.method() }`
  guard produces — so the modeled-op codegen is reused verbatim.
* `wrap_optional_receiver_method` builds `Conditional { cond: !recv.is_none(),
  then: <op>, else: undefined }`, with the result type flattened through
  `optional_chain_result_type` so a method that already returns `Optional`
  (e.g. `Map.get`) does not double-wrap. The receiver is shared by the presence
  test and the narrowed access; MIR memoizes each HIR expression, so the
  receiver is evaluated exactly once and its temporary dominates both uses
  (single-eval, matching JS `?.` semantics).
* The per-method builders (`map_has_call`, `map_get_call`, `map_mutation_call`,
  `set_contains_call`, `set_mutation_call`, `set_projection_call`, and the new
  shared `map_projection_with_receiver`) now take the pre-lowered receiver, so a
  single code path serves both the plain and the optional-chained spellings.

### Semantics note (Set/value-collection mutation)

For `Map` (runtime `SmeltRecord`, `Rc<RefCell<..>>`-shared) the narrowed clone
shares storage, so `stack?.set(k,v)` / `stack?.delete(k)` mutate the underlying
map — correct, and the es-toolkit shape. For a value-semantic `Set`
(`HashSet`), the narrowed access is an owned clone, so a mutation through an
optional receiver is not observed by the caller. This is **pre-existing**
behavior: the ordinary non-optional narrowing path (`if (s !== undefined) {
s.add(x) }`) already re-clones the owned `Option<HashSet>` on each narrowed
access and loses the write the same way. Optional `Set` reads (`s?.has(x)`)
lower correctly, and the desugar is consistent with the established narrowing
semantics. es-toolkit does not rely on optional `Set` mutation propagation.

## Tests

* Codegen regression tests (`crates/smelt-codegen-rust/src/tests/part_5_tests.rs`):
  * `emits_optional_chained_map_methods_as_guarded_modeled_ops`
  * `emits_optional_chained_set_has_as_guarded_modeled_op`
* End-to-end fixture verified with `smelt build` + generated `cargo test`
  (3 tests green): optional Map `has/set/get/delete` propagate through the
  reference-shared receiver; Map methods on an absent receiver return
  `undefined`; Set `has` reads through present/absent receivers.

## Whole-crate probe (es-toolkit)

Iterating the first-abort loop on a private copy of the es-toolkit checkout
(`isBlob.spec.ts` / `isFile.spec.ts` added to that copy's `exclude` list only —
the `globalThis.File/Blob` monkey-patch family a sibling agent owns), the build
advanced past isMatchWith and cleared these further families (fixed generally):

1. **Host-global monkey-patch (deferred / non-goal).**
   `src/predicate/isBuffer.spec.ts` does `delete global.Buffer` /
   `global.Buffer = ...`. Same family as the `globalThis.File` monkey-patch;
   excluded in the probe copy only.
2. **Array predicate callback truthiness** (`list_query.rs`, `types.rs`).
   `arr.findIndex/filter/find/some/every(cb)` where `cb` returns a non-`boolean`
   (e.g. an erased `unknown` predicate) previously errored ("array predicate
   callback must return boolean"). Now the single callback result is coerced to
   a JS truthiness test via the new `value_truthy_text` helper, matching source
   semantics.
3. **Optional field on a statically-absent receiver** (`call_runtime.rs`).
   `window?.document` (a DOM host global the non-DOM profile models as `()`)
   errored in optional-field codegen. An optional field read on a `None`/unit
   receiver now folds to the destination's absent default (`undefined`),
   short-circuiting the chain as JS does.
4. **`Array.prototype.pop` destination coercion** (`list_mutation.rs`).
   `arr.pop()` whose destination optional-inner differs from the list item type
   (e.g. a union item type) errored. The popped value is now coerced to the
   destination through the standard coercion seam; the two exact-match fast
   paths are preserved.
5. **Surplus positional call arguments** (`call.rs`).
   A call passing more positional arguments than a fixed-arity (non-rest)
   callee declares errored ("call argument has no target parameter").
   JavaScript ignores surplus positional arguments; the operands are already
   evaluated into temporaries before the call, so the extra arguments are simply
   not forwarded. `tsc` only admits this shape for erased/`any` callees, so a
   genuine over-application is still rejected upstream.

### Genuine wall (deferred, architectural)

The probe then aborts at:

> `match codegen requires all non-terminating arms to share one join block`

A `switch`/`match` whose non-terminating arms fall through to *different* join
blocks (divergent post-arm control flow) is not representable by the current
match emitter (`control_flow_match.rs::match_join`), which requires a single
shared join block. Reworking match/switch arm emission to support divergent
join targets is a structural MIR/codegen change beyond the optional-receiver
scope and is deferred. The probe copy did **not** reach `dist-smelt` emission,
so no generated-crate diagnostics/test report was produced this pass.

## Validation

* `cargo check --workspace` clean.
* `cargo clippy -p smelt-frontend-ts -p smelt-codegen-rust --lib -W
  clippy::pedantic` clean.
* `cargo test --workspace --exclude smelt-gui`: all green (no regressions;
  518 / 186 / 857 / ... passing across crates).
* `dump-hir` on `isMatchWith.ts` succeeds.
* No net increase in `SmeltUnknown`: the optional-receiver desugar replaces an
  erased `SmeltUnknown` mis-lowering with concrete modeled ops.
