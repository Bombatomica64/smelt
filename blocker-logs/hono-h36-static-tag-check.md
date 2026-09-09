# H36 — a runtime tag test against a type that already answers it

Round 13, item 3. **es-toolkit now compiles**, and its generated suite reads
**1055 passed / 4 failed** again.

## The site

`Array.isArray(values)` where `values: string[] | undefined`:

```rust
_smelt_tmp_21 = values.clone().as_ref().is_some_and(|smelt_value| matches!(smelt_value, SmeltUnknown::Array(_)));
//                                                                        ^ &SmeltList<SmeltUnknown>, not a SmeltUnknown
```

`third_party/es-toolkit/dist-smelt/src/pullAllWith.rs:76`, the one error left in
the crate after `a6f12525` cleared the seven `isBlob`/`isFile` ones. It lives in
code H28 had been deleting, which is why it only appeared last round.

## The rule

`tag_check`'s `Optional(inner)` arm narrowed a present payload with
`tag_check_raw`, which emits a `SmeltUnknown::…(_)` pattern. That pattern can
only be matched against a `SmeltUnknown`. Against a payload whose Rust type is
already concrete it does not narrow anything — it does not compile.

And it is not needed. **A `SmeltList` IS an array, on every path.** The static
type answers the test, so there is nothing to ask at run time; a hand-writing
Rust team would observe that and not write the check. What is left for an
`Option` is only PRESENCE:

```rust
_smelt_tmp_21 = values.clone().is_some();
```

`static_tag_check(ty, kind)` decides the test from the type — `Bool` answers
`typeof "boolean"`, `List`/`Tuple` answer both `Array.isArray` and `typeof
"object"` (arrays are objects in JS, which is the set `tag_check_raw`'s `Object`
arm already matches), `Function` answers `typeof "function"`, `Future` answers
both `Promise` and `Object`, and so on. `Optional` then folds to `is_some()` when
the payload satisfies the kind and to `false` when it cannot, and the bare
(non-`Option`) path folds to a constant for the same reason.

Erased shapes decline: `unknown`, an unscoped type parameter, a non-concrete
union, `never`, a generator, and an erased class — which really is a
`SmeltUnknown` at run time — all return `None` and keep the runtime match. So
does anything the helper cannot answer confidently, so declining is the safe
default rather than a guess.

`Undefined` on an `Option` also folds to `is_none()`, next to the `Null` case
that already did: `typeof x === "undefined"` is true exactly when the value is
absent.

## Coverage

Two tests in `crates/smelt-codegen-rust/src/tests/truthiness_lowering_tests.rs`,
covering both directions of one rule:

* `an_is_array_guard_on_a_concrete_list_checks_only_presence` — asserts the
  presence check appears AND that no `SmeltUnknown::Array(_)` pattern is
  emitted. That pattern is what failed to compile, so its absence is the
  non-vacuous half; verified to FAIL with the fix reverted.
* `an_is_array_guard_on_an_erased_value_keeps_its_tag_check` — an `unknown`
  operand must keep the runtime match. Green both before and after, which is
  the point: it pins the half of the rule the fix must not break.

## es-toolkit, restored

| | |
| --- | --- |
| crate compiles | **yes** (0 errors) |
| generated tests | **1055 passed / 4 failed** |
| ratchet | 32518 → **32517 (−1)**, re-snapshotted in this commit |

The −1 is this fix removing its own erased narrowing, so the baseline is
re-snapshotted as CLAUDE.md requires for a decrease; the diff is exactly that
one shape and the total.

The four failures, for the record:

* `at_spec::at_should_return_undefined_for_non_integer_indices`
* `isBrowser_spec::isbrowser_should_return_true_in_browser_environment`
* `isPlainObject_spec::isplainobject_should_return_true_for_cross_realm_plain_objects`
* `mergeWith_spec::mergewith_should_respect_null_returned_from_customizer`

## Gates

cargo test 105 result lines / 0 failures / 2658 passed (frontend 1069, codegen
1024); clippy `--lib` clean and `--all-targets` clean outside `smelt-specialize`;
end-to-end goldens byte-identical; examples SmeltUnknown avoidable 0 (+0);
remeda 1789/0; hono routers slice unchanged at 58.

Two notes on gate movement that are NOT from this commit:

* the remeda advisory reads +3 (25065 → 25068). Measured with this fix stashed
  it is also +3, so it arrived with the merge, not from here. Advisory, never
  blocks.
* **radash is gone** from `third_party/` in both the worktree and the main
  checkout, so its 84/84 could not be run this round. Removed elsewhere; not
  restored here rather than reporting a stale number.
