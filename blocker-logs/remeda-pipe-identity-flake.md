# remeda `pipe`: an identity-sequence-dependent intermittent failure — ROOT CAUSED AND FIXED

## Symptom

    remeda  uniqueBy > pipe get executed 3 times when take before uniqueBy
    remeda  uniqueBy > pipe gets executed until target length is reached
    panicked at src/map.rs: "unknown is not array"

Intermittent: measured 3 failures in 20 runs of the generated remeda suite on an
unchanged binary (`claude/generated-rust-performance` @ 61f9171).

## Root cause

**Address reuse in the emitted identity registries.**

The prelude keeps four thread-local registries keyed by the raw address of an
`Rc` allocation, and never removes an entry, because there is no drop hook to
remove it from:

* `SMELT_CALLABLE_OBJECTS`   — the callable object a typed callback was narrowed from
* `SMELT_FUNCTION_ORIGINS`   — the typed callback an erased wrapper was built from
* `SMELT_FUNCTION_IDENTITIES` — canonical JavaScript function identity
* `SMELT_FUNCTION_LENGTHS`   — `Function.prototype.length`

An `Rc` allocation is freed when its last strong handle drops, and the allocator
hands that block straight to the next allocation of the same size. A freshly
built callback landing on a dead callback's address therefore inherits the dead
callback's registry entries.

Test PARALLELISM was the variable only because `cargo test`'s thread-per-test
scheduling decides which allocations precede which on a given thread, and the
registries are `thread_local`. `--test-threads=1` happens to produce an
allocation order in which the aliasing does not bite; it hides the bug, it does
not fix it. The earlier hash-iteration-order guess was wrong, and so was the
"`pipe` publishes its lazy accumulator as an erased array" guess.

## The exact chain, from the captured backtrace

    map_134::{{closure}}                    src/map.rs:16:1645   <- panic
    SmeltErasedFunction::call
    lazy_data_last_impl::{{closure}}        lazyDataLastImpl.rs:17   (`dataLast`)
    lazy_data_last_impl::{{closure}}        lazyDataLastImpl.rs:28   (`__smelt_call`)
    process_item                            pipe.rs:304
    pipe_10                                 pipe.rs:213

Column 1645 on `map.rs:16` is the `_ => panic!` of the coercion that rebuilds
`map`'s **array** parameter, i.e. `pipe` called `dataLast(item)` — the data-last
wrapper of `map(cb)` — instead of `map`'s lazy evaluator.

How the wrong callable got there: `map_134`'s erased `lazy` adapter coerces its
argument to `Rc<dyn Fn(SmeltUnknown, f64, &SmeltList<SmeltUnknown>) -> SmeltUnknown>`
(which runs `smelt_register_callable_object`), then erases the typed result —
a callback of **the same `Rc` type**, hence the same allocation size — back to
`SmeltUnknown` through `smelt_lookup_callable_object`. When the result landed on
a recycled address, the lookup answered with a PREVIOUS operation's
`{ __smelt_call: dataLast, lazy, lazyArgs }`. `prepareLazyFunction` then kept
that object's `__smelt_call` and `pipe`'s `processItem` invoked it with
`(item, index, items)`; `dataLast` ignores arguments 2 and 3 and calls
`map(item, cb)`, routing one ITEM into the ARRAY slot.

## Fix

`smelt_retain_callable_key` reserves each keyed address with a `Weak` before the
address is stored in any registry, as a key or as a canonical-identity value
(`smelt_canonical_function_identity`). Holding a `Weak` keeps the `RcBox` block
allocated after the last strong handle drops — the value itself is still dropped,
so captured state is released — which makes the address unreusable, and therefore
makes every registry key unique to one allocation for the life of the thread.
Registry growth is unchanged in order of magnitude; these maps were already
unbounded.

## Evidence

* Baseline, unchanged binary: **17 pass / 3 fail in 20 runs**.
* With the fix: **40 pass / 0 fail in 40 runs**.
* Deterministic reproduction: `crates/smelt-codegen-rust/tests/callable_object_identity_runtime.rs`
  emits a remeda-shaped crate and appends a Rust probe that frees a registered
  callback, then allocates fresh callables of the same type. Before the fix both
  probes fail on the first candidate —
  `a fresh callback reused a registered address and inherited a dead callback's callable object`
  and the same for `SMELT_FUNCTION_ORIGINS`. After the fix no candidate ever
  reaches the address.

  A reproduction expressed purely in TypeScript was attempted and abandoned: the
  aliasing needs an exact allocator parity between the register site and the
  lookup site, and the source language gives no handle on allocation order. A
  single-threaded scan of all 174 generated modules paired with the failing test
  also found no deterministic subset.

## Consequences for the two changes this blocked

1. **Erasure sharing** (`From<SmeltList<SmeltUnknown>> for SmeltArray` via
   `with_storage`, reverted in #219). Not the cause; it only perturbed allocation
   enough to change how often the aliasing was observed. Worth retrying on top of
   this fix — with the remeda suite run at least twenty times.
2. **FxHash property keys** (es-toolkit `unique` 25.4M -> 14.5M instructions,
   `partition` 136.8M -> 121.7M). Same story. Not re-landed here.

## Still open (same defect class, not implicated in this failure)

`SMELT_LIST_IDENTITIES` and `SMELT_PROMISE_IDENTITIES` key on a source local's
storage address with the same never-removed entries. The prelude already
documents the empty-`Vec` sentinel collision as a known limitation; address reuse
is a second way those can alias, and they are not covered by the guard added
here.
