# A borrow-based ABI for generated Rust — grounding, design, and stages

## 0. The headline, stated before the design

**Since #219 the ownership ABI is no longer what the profiles are paying for.**

`SmeltList<T>` is `{ id: usize, values: Rc<RefCell<Vec<T>>> }`
(`crates/smelt-runtime/src/value/list.rs:35`). Passing one *by value* is an `Rc`
increment and a later decrement — two non-atomic adds, zero allocations. Passing
`&SmeltList<T>` instead saves exactly those two adds, and the callee must
`borrow()` anyway. **The owned `SmeltList<T>` parameter already IS the borrow.**
A borrow ABI for list-typed *function parameters* is worth approximately nothing,
and this plan does not propose it.

What the profiles pay for is **erasure**, in two forms:

1. per-element `SmeltUnknown::clone` / `drop_in_place<SmeltUnknown>` — 14.1% of
   `chunk` alone, plus refcount traffic inside `partition`'s 43% bucket;
2. functions that are generic in TypeScript being emitted non-generic, so those
   clones exist at all.

So the work splits: a genuine borrow-ABI extension (an extension of
`callback_param_is_shared_reference`, not a new mechanism), and a de-erasure fix
that is cheaper and larger.

## 1. Where the allocations come from

Counted off emitted text in `target/compat-repos/es-toolkit/dist-bench/src/`,
cross-referenced to the emitter site. Nothing was compiled; Stage 0 checks the
two inferences that carry weight.

### 1.2 `chunk`, 10,000 numbers

`dist-bench/src/chunk.rs:7-59`. With `size=7`, `n=10_000` the loop runs 1,429
times. Per iteration: 1 `Vec` malloc (the `collect`, `emitter/list.rs:457`), 1
`RcBox` malloc (`SmeltList::new`, `runtime/value/list.rs:60`), 7
`SmeltUnknown::clone`. Per op: ~2,858 mallocs, ~10,000 clones, ~10,000
`drop_in_place` + 1,429 `Rc::drop_slow` + 2,858 frees at teardown. That maps onto
the observed profile (35.0% malloc-family, 10.5% `Vec::from_iter`, 7.7%
`drop_in_place`, 6.4% `SmeltUnknown::clone`, 5.0% `Rc::drop_slow`).

Two conclusions:

* **The two allocations per output chunk are semantically required.** JS
  `arr.slice()` allocates too. No borrow ABI removes them. The only structural
  saving is collapsing `Rc<RefCell<Vec<T>>>` (RcBox + Vec buffer) into one — a
  representation change, not an ABI change.
* **`SmeltUnknown::clone` + `drop_in_place` = 14.1%, and it is pure erasure.**
  For `T = f64` both compile to nothing. This is the cost of `chunk<T>` being
  emitted as `chunk(SmeltList<SmeltUnknown>, f64)`.

Also: `arr.len()` is emitted **six times per slice expression**
(`emitter/list.rs:451-462`), each a separate `RefCell` borrow-flag round trip.
Hoisting one `let smelt_len` removes five of six.

### 1.3 `partition`, 10,000 records

`dist-bench/src/partition.rs:7-40`. This one KEPT its generics but is
instantiated at `T = SmeltUnknown`. Per op: 2 mallocs for the output lists;
10,000 `SmeltUnknown::clone` from the indexed read (`emitter/place.rs:340-347`);
another 10,000 clones + 10,000 drops from **the by-value callback item
parameter**; ~24 `realloc`s growing two ~5,000-element vectors (~650 KB memcpy,
and the large frees are what pull in `malloc_consolidate`); 10,000
`drop_in_place` at teardown.

So the 43% bucket is NOT whole-collection copies (those died with #219). It is
realloc/memcpy, drop glue, and 40,000 refcount ops from the by-value callback
parameter — the one line a borrow ABI directly removes.

### 1.4 Corpus-wide: erasure dominates the callback ABI

Over 418 emitted functions: **34 carry real Rust generics**, against ~800 TS
signatures declaring type parameters. Avoidable erasure 34,853; top shape is
`let _smelt_tmp_N: SmeltList<SmeltUnknown>` (2,245 occurrences).

First-callback-parameter census:

    221  Fn(SmeltUnknown ...     <- by value today
     39  Fn(String ...           <- by value today
     24  Fn(Vec ...
     17  Fn(T ...
     12  Fn(f64 ...
      6  Fn(SmeltList ...        <- by value (mutable/rest exclusions)
      3  Fn(&SmeltList ...       <- the borrow rule firing today

**The existing rule fires on 3 of 341.** Extending to `Type::Unknown` reaches 221
(65%); adding `Type::Str` reaches 260 (76%). A borrow ABI scoped to concrete
types would miss essentially everything.

### 1.5 Why `chunk<T>` lost its type parameter

`EmitContext::populate_generic_functions` (`emitter/mod.rs:328`) trial-renders
each candidate and keeps generics only if `renders_real_generics`
(`emitter/core.rs:321`) holds — i.e. `body_needs_erased_carrier`
(`core.rs:5032`) finds none of `ERASED_CARRIER_TOKENS` (`core.rs:5004`).

`chunk`'s body contains exactly one erased-carrier mention, and it is the
`throw new Error(...)` payload (`dist-bench/src/chunk.rs:34`). Nothing else in
`chunk` touches `T` non-opaquely.

**A single `throw` erases the entire function**, and with it 14.1% of its
instruction count. The throw payload is a genuine dynamic boundary —
`smelt_throw` returns `Box<dyn Error>` and never touches `T` — structurally
identical to the case `strip_mut_list_adapter_blocks` (`core.rs:5049`) already
exempts.

**19 non-generic es-toolkit functions contain `smelt_throw`**: after_82,
attempt_async, before_85, **chunk_2**, combinations_3, delay_103, in_range,
invariant_217, percentile_117, random_49, range_118, range_right, retry_104,
round_120, sample_size, timeout_196, trim_end, trim_start, windowed_72.

The value of a concrete `T` is already measured in `benchmarks/FINDINGS.md`:
`unique` (`T = SmeltUnknown`) 12.9 ops/s vs `unique_typed` (`T = f64`) 19.2 —
**1.49x for the same code with the tag removed.**

## 2. Constraint 1, head on: `&[T]` is not available, and is not needed

Elements live in `Rc<RefCell<Vec<T>>>`. A `&[T]` only exists through a `Ref`
guard and cannot outlive it. `Deref`/`DerefMut` were removed for this reason
(`runtime/value/list.rs:31-41`). Four routes:

1. **Name the guard, pass `&*guard` within one body.** Fails constraint 3: the
   guard then spans call sites and any aliased write panics `already borrowed` —
   a crash single-threaded JavaScript cannot produce.
2. **Return the guard (`-> Ref<'a, Vec<T>>`).** Viral through every signature,
   still panics.
3. **A "frozen list" `Rc<[T]>` for lists proven never mutated.** Needs a
   never-mutated proof across aliases and the erasure boundary. This session
   established that a syntactic aliasing analysis is unsound once clones share a
   buffer: two different locals can name one cell. Do not build this.
4. **Accept that the `Rc` handle IS the borrow.** 2 refcount ops, no allocation,
   aliasing writes stay visible. This is the answer.

**Consequence: borrows go IN, never OUT.** No stage returns a borrow, which is
what keeps named lifetimes out of generated Rust entirely.

## 3. The proposed ABI

One rule, extending `emitter/types.rs:88`:

    !function.mutable_params.contains(&index)
        && function.rest != Some(index)
        && matches!(self.mir.types.get(param),
            Some(Type::List(_) | Type::Unknown | Type::Str))   // <- the change

`synthesized_callback_param_is_shared_reference` (`types.rs:84`) takes the same
widening. The seven emit sites all consult the one predicate, which is what keeps
a closure's signature from drifting from the `dyn Fn` it is cast to.

**Case A — `map`** (`emitter/list_query.rs:289`, `:401`): the callback loses
`item.clone()`; `item` is already `&T` from `.iter()`. **No new borrow guard is
introduced** — the `Ref` on `smelt_array` already spans the callback today. One
clone and one drop removed per element.

**Case B — `slice`**: not improved by a borrow ABI at all. The receiver already
borrows (`operand_borrow_text`, `core.rs:2060`). It is improved by Stage 1
(de-erasure) and by hoisting the repeated `arr.len()`.

**Case C — `filter` chain**: the predicate call loses one clone per element; the
retained-element clone stays (it materialises the output). A
`filter(p).map(f)` chain emits two blocks each ending in `collect()` wrapped in
`Into::<SmeltList<_>>::into(..)` — 2 `Vec` + 2 `RcBox` mallocs for the
intermediate. Fusing them (Stage 6) leaves one of each.

## 4. Where lifetimes thread

**Nowhere**, and that is load-bearing rather than accidental.

* `Rc<dyn Fn(&SmeltUnknown, ..)>` desugars to a higher-ranked
  `for<'a> Fn(&'a SmeltUnknown, ..)` — exactly what a callback stored in an `Rc`
  needs. Elision produces the HRTB form automatically.
* **Closure captures**: a `&SmeltUnknown` parameter cannot be captured by an
  escaping `move` closure (E0521). Identical to the limit the list borrow already
  has, with the identical repair already implemented: the signature must not
  change, so the body materialises its own owned copy outside the `'static`
  block. One copy per *call*, not per element.
* **The erasure boundary**: typed→erased hands `&args[0]` out of an owned `Vec`
  that outlives the call; erased→typed clones into a `Vec` as today. The prelude
  already carries `IntoSmeltUnknown for &SmeltUnknown`.
* **A function returning a list derived from its argument** needs no lifetime —
  every return is an owned `Rc` handle. This is the case that would demand
  `fn f<'a>(xs: &'a [T]) -> &'a [T]` in a hand-written port, and §2 rules it out.

**The rule that holds the line: the moment a stage needs a named lifetime, it has
drifted into `&[T]` territory and should be rejected.**

## 6. Stages

**Stage 0 — measure, no code.** (a) Add `chunk_typed`/`partition_typed` bench
cases at `T = f64` and callgrind them against the erased instantiations, to price
§1.2's 14.1% claim. (b) Callgrind `partition` with the `item.clone()` at
`partition.rs:26` hand-ablated, to price Stage 3. (c) Check whether LLVM elides
the dead `Rc::new` at `difference.rs:10`. This is the antidote to reasoning about
emitted text; two of this session's conclusions were wrong that way.

**Stage 1 — exempt the throw payload from the erased-carrier check.** Strip
`smelt_throw(..)` argument expressions in `body_needs_erased_carrier`
(`core.rs:5032`) using the same brace-balanced removal
`strip_mut_list_adapter_blocks` (`core.rs:5049`) already uses. Gates: the usual
set plus `smelt rust-diagnostics` on all three generated crates (19 functions
change signature). Expected: 19 es-toolkit functions regain `<T>` incl. `chunk`;
~1.4-1.5x for typed callers by the measured `unique`/`unique_typed` precedent;
avoidable erasure **falls** (re-snapshot in the same commit, which the ratchet
rules permit for decreases). **Honest caveat: the existing bench rows will not
move**, because they instantiate at `T = SmeltUnknown`; the win is visible only
on the `_typed` rows Stage 0 adds. Smallest diff in the plan, largest
per-function effect.

**Stage 2 — `Type::Str` callback params by shared reference.** 39 sites. Before
`Unknown` because the adapter surface is far smaller and `String::clone` is a
genuine malloc+memcpy per element — the cheap proof that the predicate really is
single-sourced before the 221-site change rides on that assumption.

**Stage 3 — `Type::Unknown` callback params by shared reference.** 221 sites.
The main borrow win: one clone and one drop removed per element on every
callback-driven case. In `partition` that is 40,000 refcount ops per op.
Estimated 10-20% on partition/group_by/count_by/unique_by; Stage 0(b) turns the
estimate into a number first. Gates add a runtime fixture asserting a
self-writing callback's behaviour is unchanged, and a diffed es-toolkit failure
SET (not a pass count).

**Stage 4 — Dict/Set/class callback params.** Measure, then probably skip: all
are `Rc`-backed, so by-value is 2 refcount ops. The `types.rs:68` comment
promising Dict and Set "should follow" predates #219 and is stale.

**Stage 5 — free-function parameter ABI.** `String` parameters only, or skip.
For lists the saving is 2 refcount ops per call against a crate-wide ABI break.

**Stage 6 — chain fusion.** At MIR level: a `Type::List` temporary produced by a
list-transform op, read exactly once, by another list-transform op in the same
block, never erased and never identity-compared, stays a Rust iterator instead of
materialising. Removes 1 `Vec` + 1 `RcBox` malloc + n element moves per link.
Condition (d) matters because `smelt_list_identity` keys an erased array's
identity on the live `Vec`'s address, so single-use in MIR is necessary but not
sufficient. This is where the remaining `Vec::from_iter` (10.5% of `chunk`) lives.

## 7. What I would not do

1. **Expose `&[T]` from `SmeltList` in any form** — §2.
2. **A "frozen list" `Rc<[T]>` representation** — needs an unsound aliasing proof.
3. **Named lifetimes in generated signatures** — every motivating case is
   answered by the `Rc` handle.
4. **Borrowed return types** — borrows go in, never out.
5. **Free-function *list* parameters as `&SmeltList<T>`** — the biggest-looking
   change in the "borrow ABI" framing is worth the least.
6. **Acting on `benchmarks/FINDINGS.md` item 5 as written** — it blames
   `SmeltJsMap::get` deep-cloning its `SmeltList` value; post-#219 that clone is
   an `Rc` bump. Stale; re-measure `group_by` first. Item 1 is likewise already
   fixed by `operand_borrow_text` plus #219.
7. **Snapshotting the array in `list_callback_iteration_parts`** to make
   self-writing callbacks safe — one whole-array copy per `map`/`filter` call to
   fix a shape that appears nowhere in the three corpora.

## 8. Two correctness notes found while reading

* **`Array(n)` with a reference element type produces n aliases of ONE array.**
  `list_from_length_text` (`emitter/list_query.rs:567-587`) emits
  `vec![<hole>; n]`, and post-#219 `Clone` on a `SmeltList`/`SmeltObject`/
  `SmeltJsSet` hole shares the buffer. Visible at `dist-bench/src/chunk.rs:44`.
  In `chunk` every slot is overwritten before it is read, so it is invisible
  there; and a program that observed it would have thrown in JavaScript, where
  `Array(3)[0]` is `undefined`. **Low severity, but it is a #219 regression.**
  Fix: `(0..n).map(|_| <hole>).collect()` when the element type is a reference
  type. Tier: `list_reference_semantics_runtime`. Note `list_repeat_text`
  (`list_query.rs:590`) is *correct* with `vec![x; n]` — `Array(n).fill(a)`
  genuinely is n aliases of one array in JavaScript.
* **`arr.len()` emitted six times per slice expression** — see §1.2.

## Summary

| # | stage | blast radius | expected effect |
|---|---|---|---|
| 0 | measure `_typed` instantiations + ablate `partition`'s `item.clone()` | none | prices stages 1 and 3 before they are built |
| 1 | exempt `smelt_throw` payloads from the erased-carrier check | one predicate | 19 fns regain `<T>` incl. `chunk`; ~1.4-1.5x for typed callers; avoidable erasure **falls** |
| 2 | `Type::Str` callback params by `&` | 39 sites | one String malloc+free per element; proves the predicate is single-sourced |
| 3 | `Type::Unknown` callback params by `&` | 221 sites | the main borrow win; est. 10-20% on partition/group_by/count_by/unique_by |
| 4 | Dict/Set/class callback params | moderate | ~nothing post-#219 — measure, then skip |
| 5 | free-function params by `&` | crate-wide | String only; lists are worth 2 refcount ops |
| 6 | chain fusion | MIR + list emitters | halves allocations per chain link |

The uncomfortable part, which is the point: the biggest single win (Stage 1) is
not a borrow at all, and the largest-sounding borrow change (Stage 5, list
parameters by reference) is worth two refcount ops.
