# es-toolkit did not compile: a callable moved out of a `Ref` guard

Found in round 10 while running the corpus gates, introduced in round 9 by H17.

## What it was

H17 (`blocker-logs/hono-h17-private-field-capture.md`) fixed an "already
borrowed" panic: a callee read out of a `RefCell` — a reference class's
function-typed field, or a shared closure capture — used to be called in place,
so the read's `Ref` guard was still alive while the callee ran, and a
class-field arrow whose body mutates the same cell panicked. The fix binds the
callable first, which drops the guard at the end of the `let`:

```rust
{ let smelt_callable = <callee>; (smelt_callable)(<args>) }
```

That is right for the class-field case, whose callee text is already an owned
clone (`recv.0.borrow().f.clone()`). It is not right for a SHARED CLOSURE
CAPTURE, whose callee text is `(*smelt_capture_recursive.borrow())`: the binding
then *moves* an `Rc<dyn Fn ..>` out of a `Ref` deref, and `Rc` is not `Copy`.

```
error[E0507]: cannot move out of dereference of `std::cell::Ref<'_, Rc<dyn Fn(&SmeltList<SmeltUnknown>, f64) -> SmeltUnknown>>`
  --> src/flatten_1.rs:74
```

Two sites in es-toolkit (`flatten`, `flattenDeep` — a self-recursive arrow),
which meant the **whole probe crate failed to compile**, so the corpus's
generated tests had not run at all since round 9. Nothing in the SmeltUnknown
ratchet or the blocker count notices that: both are textual scans of emitted
source.

## The rule

Clone out of the guard rather than moving out of it:

```rust
{ let smelt_callable = ::std::clone::Clone::clone(&<callee>); (smelt_callable)(<args>) }
```

An `Rc` clone is a refcount bump; the `let` still drops the guard before the
call, so H17's property is kept. A callee text that is already an owned clone
pays one further refcount and nothing else. Both emission sites — `call.rs`
(typed indirect call) and `call_runtime.rs` (erased call) — carry the same
spelling, since they answer the same question about the same receiver shapes.

## Where the corpus stands now

| | before | after |
| --- | --- | --- |
| es-toolkit generated crate | **does not compile** (2 × E0507) | compiles |
| es-toolkit generated tests | not runnable | **1055 passed / 4 failed** |

The four failures are pre-existing gaps unrelated to this fix, and each names
its own reason:

* `isBrowser should return true in browser environment` — the profile is non-DOM
  by design.
* `isPlainObject should return true for cross-realm plain objects` — needs `vm`
  realms.
* `at should return undefined for non-integer indices`.
* `mergeWith should respect null returned from customizer`.

The first two are profile decisions; the last two are real families for a later
round, and they are now visible because the crate runs.

## Test

`crates/smelt-codegen-rust/src/tests/part_7_tests.rs` —
`a_recursive_capture_callee_is_cloned_out_of_its_borrow_guard`: a self-recursive
arrow (the reduced `flatten` shape) must never emit `let smelt_callable = (*`.
