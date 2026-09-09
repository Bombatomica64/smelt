# The router slice's 50 E0308s, bucketed — and the three general rules that
# removed 46 of them

Round 26, item 2. The slice's `cargo check` went from **56 errors to 10**, and
its E0308 count from **50 to 4** (the four that remain are H42, deferred).

## Before

| n | code | shape |
| ---: | --- | --- |
| 40 | E0308 | expected `SmeltUnion156`, found `SmeltUnknown` |
| 4 | E0308 | expected `Option<(String, f64)>`, found `Option<SmeltUnknown>` |
| 2 | E0308 | expected type parameter `T`, found `SmeltUnknown` |
| 2 | E0308 | expected `Option<()>`, found `Option<Result<(), Box<dyn Error>>>` |
| 2 | E0308 | expected `SmeltList<(T, SmeltRecord<String, f64>)>`, found `SmeltList<(SmeltUnknown, …)>` |
| 2 | E0424 | `self` bound as a local variable |
| 2 | E0277 | `SmeltRecord<String, SmeltRecord<String, SmeltList<(…)>>>` from an iterator whose items carry `T` |
| 1 | E0283 | annotations needed for `__smelt_anon_class_3050<_>` |
| 1 | E0615 | `build_all_matchers` read as a field |

All 46 fixed errors came from **one emit site** — the optional-chain read and
call in `emitter/optional_access.rs` — and three rules it was missing.

## Rule 1 — a generated union reaches runtime narrowing through its adapter

A concrete generated union (`SmeltUnion156`) is its own Rust enum, not a
`SmeltUnknown`, so matching `SmeltUnknown::String(..) | Array(..) | Object(..)`
arms against it cannot compile. The non-optional index read already erased its
receiver first (`erase_concrete_union_text`, `place.rs`'s `Type::Union` arm);
the OPTIONAL read did it for the index operand but not for the receiver.

**40 of the slice's 56 errors were that one missing conversion.**

## Rule 2 — the erased element is converted to the read's result type

The arms of that narrowing produce an erased element. Only `String` results
were converted; every other concrete result took the erased value as-is, so a
read declared `Option<(String, f64)>` was handed an `Option<SmeltUnknown>`.
Any non-`String`, non-erased result now goes through the same
`value_at_type_text` boundary conversion the rest of the emitter uses (4
errors).

## Rule 3 — a throwing method propagates out of an optional chain

`registry?.insert(k, v)` calls a method that can throw. Such a method returns a
`Result` in the generated Rust, and `?` cannot cross a closure boundary, so
`opt.map(|v| v.insert(..))` answered `Option<Result<(), _>>` into an
`Option<()>` slot. The chain is now a `match`, which puts the call in the
enclosing function's own body where `?` propagates (2 errors).

That exposed a second half in the MIR pass: **a method called through an
optional chain is an `Rvalue`, not a call terminator**, so
`propagate_throwing_functions` never saw it and the enclosing function was not
marked `can_throw` — the `?` then sat in a function that returns no `Result`.
The pass now resolves an `Rvalue::OptionalMethod` against the class table, the
same way the emitter resolves it when rendering the call.

## After

| n | code | shape | family |
| ---: | --- | --- | --- |
| 2 | E0424 | `self` bound as a local variable | H57 |
| 2 | E0277 | record from an iterator whose items carry `T` | H42 |
| 2 | E0308 | expected type parameter `T`, found `SmeltUnknown` | H42 |
| 2 | E0308 | `SmeltList<(T, …)>` vs `SmeltList<(SmeltUnknown, …)>` | H42 |
| 1 | E0283 | annotations needed for `__smelt_anon_class_3050<_>` | round-26 item 3 |
| 1 | E0615 | `build_all_matchers` read as a field | H44 |

## Regression guard

`examples/typescript/end-to-end/80_optional_chain_union_and_throw`, verified to
fail with the emitter change reverted — and to fail with **all three** families
at once (`expected SmeltUnion8, found SmeltUnknown`,
`Option<(String, f64)>` vs `Option<SmeltUnknown>`, and `Option<()>` vs
`Option<Result<..>>`), which is why one fixture covers all three rules.

One incidental measurement while keeping the examples invariant at avoidable 0:
a tuple-list literal passed directly into a union-typed parameter erases
(`SmeltList<SmeltUnknown>`, 8 lines), while the same literal bound to a typed
local first is injected as the union arm (`SmeltUnion8::M1(pairs)`). The
fixture binds it, and the difference is worth a rule of its own later.
