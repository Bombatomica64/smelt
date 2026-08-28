# `SmeltList<T>`: shared backing buffer

## What changed

`SmeltList<T>` was `{ id: usize, values: Vec<T> }` with a `Clone` that preserved
`id` while deep-copying `values` — identity and storage disagreed, so a cloned
handle claimed to be the *same* JavaScript array as the original while writing to
a different buffer. It is now

```rust
pub struct SmeltList<T> { id: usize, values: Rc<RefCell<Vec<T>>> }
```

with `Clone` bumping the `Rc` and keeping the id, exactly as `SmeltArray`,
`SmeltObject`, and `SmeltJsMap` already do. `fresh_copy` (`[...a]`, `slice()`)
mints a new id *and* a new buffer, which is what keeps sharing from becoming
over-sharing.

`Deref`/`DerefMut` to `Vec<T>` are gone — the elements live behind a `RefCell`,
so there is no `&Vec<T>` to hand out that could outlive the borrow (the argument
the erased `SmeltArray` already makes, and the one that kept `SmeltJsSet`
copy-on-write). In their place the type exposes `borrow()`/`borrow_mut()`, and
the emitter renders a list receiver through two new helpers in
`emitter/mod.rs`: `list_read_text` (`x.borrow()`) and `list_write_text`
(`x.borrow_mut()`). Because those guards deref to `Vec<T>`, every emitted chain
kept its existing shape and its `&T` item types; only the receiver text moved.
`len`/`is_empty` stayed inherent on `SmeltList` so the very common
`x.len()`-inside-an-index-argument shape keeps taking its own short-lived borrow.

## What now holds

`crates/smelt-codegen-rust/tests/list_reference_semantics_runtime.rs` lowers and
*runs* six TypeScript fixtures. Before this change 2 of 6 passed; all 6 pass now:

| case | before | after |
| --- | --- | --- |
| `const b = a; b.push(x)` seen through `a` | pass | pass |
| callee mutates the caller's array | pass | pass |
| array in an object field, mutated through a read-back handle | **fail** | pass |
| array stored in a `Map`, mutated through a read-back handle | **fail** | pass |
| array nested in another array | **fail** | pass |
| `[...a]` / `a.slice()` are independent copies | **fail** (see below) | pass |

## What does NOT hold

### `===` between two typed arrays

`a === [...a]` still reads `true`. This is independent of storage and predates
this change: the emitter lowers a source `===` between two `Type::List` operands
to `BinOp::JsStrictEq`, which falls through to `SmeltList`'s *structural*
`PartialEq` instead of the id comparison that `strict_identity_text` /
`reference_identity_text` (`emitter/binary_ops.rs`) already implement for
`BinOp::StrictEq`. Two equal-contents arrays therefore compare `===` even when
they are separate arrays with separate buffers, and `a === b` on genuine aliases
happens to answer correctly for the wrong reason. Fixing it means extending the
identity path to `JsStrictEq`, which changes `===`/`toBe` answers across the
library corpora and is a separate change with its own gate run.

### Erasing a typed list to `SmeltUnknown` — RESOLVED

Landed: both `From<SmeltList<SmeltUnknown>> for SmeltArray` and its `&` form now
use `with_storage`, so erasure is a refcount bump rather than a copy.

The remeda `pipe` failure recorded here as the blocker does **not** reproduce on
the merged tree (remeda stays 1789/0). It was an artifact of the pre-merge base:
the merge made materializing a pushed item unconditional, which removed the
aliased read-and-write-in-one-expression shape that a shared buffer turned into a
live borrow conflict.

Result: es-toolkit 954/105 -> **956/103**, failure sets diffed, zero new failures.
The two fixed are `isEqualWith should compare arrays with circular references when
customizer returns undefined` and its transitive-equivalence sibling — a circular
array (`a[0] = a`) is only representable when the erased element IS the array, so
those two tests are the sharpest available evidence that sharing is the correct
semantics rather than merely the faster one.

Throughput is neutral. A controlled A/B on one machine state gave chunk 0.99x,
group_by 1.00x, unique 0.90x. Note for anyone reading numbers off this suite: the
same binary measured `chunk` at both 6,411 and 4,746 ops/s across runs on this
box, so cross-run variance is ~25% and any reported delta below roughly 1.3x
should be treated as noise unless it comes from a controlled A/B.

### Callback iteration holds a read borrow across the callback

`list_callback_iteration_parts` and the `reduce` path iterate
`list.borrow().iter()`, so the `Ref` guard spans the user callback. A callback
that only reads the array it is iterating is fine (concurrent shared borrows are
allowed) and that is every case in the remeda / es-toolkit / radash corpora,
because a callback that *wrote* the array did not compile at all when the
elements were an inline `Vec` (E0502). A callback that writes the array it is
iterating — legal JavaScript — now panics `already borrowed` instead of failing
to compile. The alternative is a snapshot per call, i.e. one copy of the whole
array per `map`/`filter`/`forEach`, which is the cost this iteration shape exists
to avoid. Same reasoning applies to a comparator that reads the array it sorts.

### The write-back machinery is still there

`ListAliasOrigin` in `emitter/list_mutation.rs` — which tracks that a list local
came from `obj.field` or `base[key]` and re-inserts it after a push — was *not*
removed. It is now redundant for the typed-list-through-typed-container cases
that the runtime tests cover, but its two remaining arms write back into an
**erased** base (`Type::Unknown`/`Union`/`TypeParam` object, and a `Dict` whose
value type is the list). The first of those crosses the erasure boundary above,
which still copies, so removing it needs that stage landed first. Removing it is
a real win — it costs a full copy of the collection per mutation — and it should
be the stage right after erasure sharing.

## Staged plan for the remainder

1. ~~**Erasure sharing.**~~ DONE — see the RESOLVED section above.
2. **Retire `ListAliasOrigin`.** Now unblocked: the erased-base write-back arms
   are redundant; delete `list_alias_origin*` and the two special-cased push
   paths in `list_mutation.rs`, and confirm the object/dict fixtures in the
   runtime tier still pass.
3. **`===` on typed arrays.** Route `BinOp::JsStrictEq` through
   `strict_identity_text` for reference types, then re-run every library gate —
   this changes `toBe` answers, so expect movement in both directions.
4. **Callback-iteration borrow.** Decide between the current read borrow (fast,
   panics on a self-writing callback) and a per-call snapshot (safe, one copy per
   call). A middle option is to snapshot only when the callback's captures
   include the receiver, which the emitter can see.

## Measured gates (this branch vs a7a04d3)

| gate | before | after |
| --- | --- | --- |
| `cargo test --lib` | green | 907 / 205 / 988 / 4 / 13 / **29** / 45 / 25 / 37, 0 failed (smelt-runtime 27 → 29: two new list tests) |
| `cargo test -p smelt-transpiler` | green | 40 / 13 / 10 / 27 / 1, 0 failed |
| remeda generated tests | 1789 passed, 0 failed | 1789 passed, 0 failed |
| radash generated tests | 84 passed, 0 failed | 84 passed, 0 failed |
| es-toolkit generated tests | 952 passed, 107 failed | **954 passed, 105 failed** — no new failures; `unzip should unzip arrays correctly` and `unzip should handle arrays of different lengths` now pass |
| `map_lookup_runtime` / `set_membership_runtime` | 5 / 4 | 5 / 4 |
| `list_reference_semantics_runtime` (new) | 2 of 6 | 6 of 6 |
| `cargo clippy --lib` warnings | 136 | 135 |
| avoidable erasure (examples) | 0 | 0 |
| avoidable erasure (es-toolkit) | 34854 | 34854 (runtime prelude 3338 → 3354) |

`cargo clippy --lib` also reports 9 pre-existing *errors* on this branch's base
(4 in `smelt-frontend-ts`, 3 in `smelt-codegen-rust` files this change does not
touch); the warning counts above are what the run emits before it aborts.

## Benchmarks (es-toolkit, `--repeats 3`, ops/sec, Rust side)

Every checksum is byte-identical before and after.

| case | before | after | ratio |
| --- | ---: | ---: | ---: |
| group_by | 3.902 | 3.928 | 1.007x |
| count_by | 300.205 | 296.981 | 0.989x |
| chunk | 6412.922 | 6096.625 | 0.951x |
| flatten | 2109.024 | 2108.186 | 1.000x |
| partition | 559.629 | 529.927 | 0.947x |
| unique | 1167.792 | 1183.095 | 1.013x |

Flat to ~5% slower on the array-shape-heavy cases (`chunk`, `partition`), which
is the `RefCell` borrow flag plus the extra pointer hop on each element read;
`Clone` on a list is now O(1) instead of a full copy, which is why `unique` and
`group_by` come out slightly ahead. Nothing here moved outside a few percent —
the big win the shared buffer unlocks (dropping the per-mutation write-back copy
in `list_mutation.rs`) is stage 2 above and is not in these numbers.
