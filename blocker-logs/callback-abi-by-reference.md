# Callback ABI: list-typed callback parameters are passed by shared reference

Landed. This file replaces the preserved `callback-abi-by-reference.patch`, which existed only
because the change was measured, found correct, and left unlanded with remeda red.

## The defect

A JavaScript array callback receives the array itself as its third argument, and in JavaScript that
costs nothing — it is the same object. Lowered by value it costs a full `SmeltList` deep copy on
*every element*, so every `map`/`filter`/`groupBy`/`sumBy`/`reduce` over n elements copied O(n^2).

## The rule

`FunctionEmitter::callback_param_is_shared_reference` (`emitter/types.rs`) — a callback parameter is
`&T` when

* it is not in `FunctionType::mutable_params` (that axis already owns the `&mut` spelling), and
* it is not the `rest` parameter (`...args` is *built* by the callee-side adapter from the erased
  argument vector, so there is nothing to borrow and the erased adapters need to own the elements),
  and
* its type is `Type::List(_)`.

Deliberately a rule about the **type**, not about any library's callback: it fires wherever a
callback declares a list parameter. Dict/Set parameters have the same argument-side cost and should
follow, but each needs its own pass over the erased-callback adapters first.

## Where the ABI is spelled

Seven emitter sites plus prelude impls, all going through the one predicate so a closure's signature
cannot drift from the `dyn Fn` it is cast to:

| site | what it renders |
| --- | --- |
| `types.rs::function_type_param_text` | the `dyn Fn(&SmeltList<..>)` spelling |
| `types.rs::callback_param_type` (`MutablePrefix::Ignore`) | adapter parameter declarations — `Ignore` drops only the `&mut` axis |
| `closures.rs::closure_text_with_extra_params` | closure parameter declarations (both the escaping-default path and the main path) |
| `call.rs::indirect_call_args_text` | arguments of a call through a function value |
| `call_runtime.rs` (plain closure-call arm) | arguments of a direct closure call |
| `core.rs::function_args_from_smelt_args_text` | the erased `Vec<SmeltUnknown>` -> typed adapter |
| `core.rs` callback adapter (`forwarded`) | arguments the arity/type adapter forwards |
| `list_query.rs` array-callback snapshot + `list_reduce_text` | the JS element/index/array arguments |
| `lib.rs` prelude | `From<&SmeltList<SmeltUnknown>> for SmeltArray`, `From<&SmeltList<T>> for Vec<T>`, `IntoSmeltUnknown for &SmeltUnknown` |

## Where a borrow cannot be used, and what happens instead

The by-reference decision is made on the callback **type**, but whether a *body* can live with a
borrow is a property of the individual closure. Two shapes cannot:

1. The body needs a **mutable owned** binding — it rebinds the parameter, or forwards it into a
   `&mut` slot (remeda `clone.ts`: `cloneImplementation(x, refFrom, refTo)`).
   `local_binding_needs_mut` already answers this.
2. The body is moved into a **`'static` block** — an `async move` future or a generator producer. A
   borrow of the caller's argument does not live that long (E0521).

In both cases the signature must not change — it has to keep matching the `dyn Fn` — so the body
materializes its own owned copy from the reference, emitted *outside* any `async move`/generator
block. That copy is the value the by-value ABI used to hand the body, so a parameter rebind and a
mutation through an owned parameter copy stay invisible to the caller exactly as before
(`parameter_needs_mutable_reference_in` is what gives the caller-visible case its own `&mut` ABI).
It is one copy per **call**, only in bodies that need it, never per element. The same repair covers
the async *adapter* closures in `core.rs`.

## The erase path

`erase_value_text`'s list arms bound `let smelt_l = <text>; ... smelt_l.into_iter()`, and
`(&SmeltList<T>).into_iter()` yields `&T`, which a primitive element wrap (`value as f64`) cannot
consume — no trait impl can reach a primitive cast. The arms now take the elements through
`Into<Vec<_>>`: `From<SmeltList<T>> for Vec<T>` **moves** the backing storage and
`From<&SmeltList<T>> for Vec<T>` clones it, so Rust's own impl selection does the narrowing. The
owned path stays copy-free; only the borrowed path pays a clone, which erasing to an owned
`SmeltArray` requires anyway.

## Payoff

Re-measured end to end on this machine: the same six bench cases built from the base-commit emitter
and from this one (`benchmarks/prepare.py --only es-toolkit`, then
`SMELT_BENCH_WARMUP_MS=300 SMELT_BENCH_MEASURE_MS=1500 SMELT_BENCH_MAX_MS=6000
SMELT_BENCH_MIN_SAMPLES=5 es_toolkit_bench run <case>`). Every checksum is identical across the two
builds, which is the correctness proof.

    case        before      after      ratio    checksum
    partition    0.534    528-542     ~1000x    2213602312
    unique_by    0.515    354-372      ~700x    3475094833
    count_by     0.545    317-339      ~600x     948281954
    group_by     0.427   3.82-3.96       ~9x    3496415674
    sum_by     771-815        810        1.0x   2835870791   (control)
    sort_by  5.00-5.14       5.10        1.0x   2119849330   (control)

`group_by` landing near the earlier ablation's 7.7x is the cross-check that the mechanism is
understood; what remains in that row is a different defect. The two controls declare no array
callback parameter and do not move — worth knowing, because a first `sort_by` reading of 4.25 looked
like a 17% regression and was pure machine load: repeated runs of the BASE build spanned 5.00-5.14
and the after build lands at 5.10.

## Gates

remeda 1789/0, es-toolkit unchanged, both SmeltUnknown ratchets pass, `cargo test --lib` green.
Four snapshots and three behaviour tests moved; one of those,
`array_callback_third_array_parameter_snapshots_once`, had asserted `smelt_array.clone()` — the
per-element copy its own name says must not happen.
