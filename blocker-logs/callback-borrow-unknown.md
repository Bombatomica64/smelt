# Widening the callback by-shared-reference ABI

## What changed

`callback_param_is_shared_reference` decided the callback ABI from the source
`Type` variant and fired only for `Type::List`. It now decides from the
parameter's **rendered Rust type**, and answers `true` for anything that renders
`SmeltUnknown` plus, uniformly, every `Type::TypeParam`.

## Why the rendered type, not the source variant

Several source spellings share one Rust type. `unknown`, a concrete union and
`never` all render `SmeltUnknown`, and the emitter freely assigns a value of one
to a slot spelled by another — in Rust they ARE the same type. Deciding per
source variant gives one Rust type two ABIs, and every such assignment becomes
an E0308. The case that forced it: es-toolkit's `matches` returns
`Rc<dyn Fn(<union>) -> bool>` and `dropWhile` binds that to a parameter declared
`(value: unknown) => boolean`.

A type parameter is answered uniformly rather than by rendering. Whether `T`
renders as `T` or erases to `SmeltUnknown` depends on which scope renders it,
and a declaring function and its callers do not share one:
`dropRightWhile<T>` declares `Fn(T, ..)` while a caller with no `T` in scope
sees `SmeltUnknown`. `&T` instantiates to `&SmeltUnknown` either way, so
answering `true` unconditionally is the only substitution-stable answer. It also
removes a per-element `T` clone from every generic callback.

## The seams this reached

Only lists took the by-reference path before, so every seam below was
unreachable and none of them asked the rule:

| seam | file | symptom if unfixed |
| --- | --- | --- |
| erased argument vector | `coercion.rs` `unknown_function_call_args_text` | `Vec<&SmeltUnknown>` inferred, every later push E0308 |
| forwarded rest list | `core.rs` (`smelt_forwarded_args`) | `SmeltList<&SmeltUnknown>`, `Into<Vec<_>>` unresolved (E0277) |
| erased-call arg ladder | `core.rs` `function_args_from_smelt_args_text` | two callers, two callee ABIs — now an explicit `ErasedCallTargetAbi` |
| array-from mapper | `list_query.rs` | E0308 on the synthesized `SmeltUnknown::Null` |
| sort comparator | `list_mutation.rs` | E0308 on `left.clone()` |
| promise continuation | `call.rs` `promise_callback_invocation_with` | E0308 on the erased resolved value |
| promise `reject` | `call.rs` | synthesized `reject` did not follow `resolve` |
| arity padding | `call_runtime.rs`, `call.rs` | es-toolkit `isEqualWith`: six params, four padded |
| spread positionals | `call_runtime.rs` | remeda `when` |
| `piped_` wrapper | `call.rs` | rendered bare types against a `&` `dyn Fn` (E0631) |
| `Type::Future` coercion | `coercion.rs` | borrow named inside a `'static` `async move` (E0521) |
| erased `flat` | `list.rs` | match arms moved out of a shared reference (E0507) |

## Measurement

`valgrind --tool=callgrind`, es-toolkit bench crate, N = 10,000. Instructions
per operation, isolated as the slope between a 1-sample and a 21-sample run so
process startup and data construction cancel. Callgrind counts instructions, so
these numbers carry no machine noise.

| case | base (`1d53fd6`) | this branch | delta |
| --- | ---: | ---: | ---: |
| partition | 4,523,914 | 4,028,875 | **-10.9%** |
| unique_by | 10,984,958 | 10,437,451 | **-5.0%** |
| group_by | 19,371,082 | 18,893,634 | **-2.5%** |
| count_by | 15,988,434 | 15,750,960 | **-1.5%** |
| unique | 4,446,677 | 4,448,410 | +0.0% |
| chunk | 2,377,525 | 2,377,536 | +0.0% |

`unique` and `chunk` are the controls: neither takes an unknown-typed callback
in its hot path, and neither moved. Result checksums are byte-identical to the
base binary on all eight cases that have them (`partition`, `unique`,
`group_by`, `chunk`, `count_by`, `unique_by`, `sum_by`, `flatten`).

The plan estimated 10-20% on `partition`/`group_by`/`count_by`/`unique_by`.
`partition` landed in that range; the other three came in below it, because
their cost is dominated by hashing and map insertion rather than by the callback
argument.

## The provenance seam, and the gate that caught it

Two more sites read a callback's parameter ABI off the wrong description of the
same function. Both were caught by CI's `compile_corpus` tier, which a plain
`cargo test --all-targets` does **not** run -- it needs
`cargo test -p smelt-codegen-rust --test compile_corpus -- --ignored`. Add that
to the gate list for anything that touches a callback signature.

**A monomorphized generic loses the by-reference marker.** `make<T>(x: T) => (v:
T) => boolean` emits as `fn make<T>(x: T) -> Rc<dyn Fn(&T) -> bool>` and the call
site monomorphizes it to `Rc<dyn Fn(&f64) -> bool>`; the destination local's MIR
type is the instantiated `(v: number) => boolean`, which no longer records that
its parameter came from a type parameter. `emitted_call_result_function_type`
answers the ABI question from the callee's declaration instead -- restricted to
callees emitted with real generics, because that is the only case where the
rendered ABI can outrun MIR. An erased callee renders its return in MIR's own
vocabulary and the rendered-value adapter has already reshaped the value;
firing there too over-borrowed an already-adapted value.

**The rendered-text adapter never asked the rule for its forwarded arguments**,
though it did for its own declarations. Both halves now go through the same
helpers.

## Gates

- internal suite green on every target; clippy 144, unchanged from the base
- remeda 1789 passed / 0 failed, over 15 multi-threaded runs (the `pipe`
  identity flake reproduces about 1 run in 6, so a single green run proves
  nothing — see `remeda-pipe-identity-flake.md`)
- radash 84 / 0
- es-toolkit: probe blockers 0; library and tests compile with zero errors;
  954 passed / 105 failed with a **byte-identical failure set** to the base
- SmeltUnknown ratchets: examples avoidable 0 (hard invariant held); es-toolkit
  avoidable 34,851 vs baseline 34,852 (-1)
