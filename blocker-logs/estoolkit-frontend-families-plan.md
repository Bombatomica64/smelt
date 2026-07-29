# es-toolkit frontend families — design plan (families 2, 3, 5, 10)

Design plan for the es-toolkit compatibility families that the ranked analysis in
`blocker-logs/estoolkit-runtime-current.md` flagged as needing frontend/feature
work rather than bounded codegen patches.

- Measured at Smelt `bfb68f18`, es-toolkit ref `e008a2818cd8d07469a5cc12ee0c02405d523e07`.
- Runtime suite: **789 passed / 270 failed**. Remeda guard: **1789 / 0**.
  SmeltUnknown avoidable erasure: **35846** (hard ratchet, may not rise).
- Family 7 (exception-payload ABI) is being implemented in parallel and is
  assumed to land; it is not planned here.
- **This plan was produced without compiling anything.** Every claim below is
  grounded in the Smelt source and in the committed generated Rust under
  `third_party/es-toolkit/dist-smelt/`. Claims that could only be settled by
  building are collected in "Open questions" and are marked as unverified.

## Executive summary

Three of the four recorded root-cause hypotheses are **wrong or materially
incomplete**. Reading the actual generated Rust changes the plan substantially,
and mostly in the cheap direction:

| Family | Tests | Recorded hypothesis | Verdict |
|---|---:|---|---|
| 2(a) clone exotic reps | ~23 | "missing host/exotic reps — stdlib surface, one rep at a time, not a bounded patch" | **Partly corrected.** The dominant single cause is not missing reps but **prototype reflection**: `Object.create(proto)` lowers to the identity of `proto`, and `Object.getPrototypeOf` on a plain object returns a *string sentinel* that the record coercion then char-enumerates. One general fix, not N reps. |
| 2(b) list reference identity | ~3 (+12 via family 10) | "`SmeltList` lacks the `Rc<RefCell<..>>` reference semantics `SmeltJsMap` already got" | **Corrected in scope.** The right container to lift is the **erased `SmeltArray`**, not the typed `SmeltList`. `SmeltList` carries the full `Vec` API through `Deref`/`DerefMut`/`Index` and is used at ~9,500 sites in one generated crate; `SmeltArray` has a 30-site emitter surface. |
| 3 isEqual/isEqualWith | 25 | "stacked value-identity semantics, blocked behind family 2, no single lever" | **Confirmed**, and the dependency is sharper than recorded: the blocker is family 2(a)'s `.constructor`/prototype lever plus `.valueOf()` on boxed primitives, not family 2(b). |
| 5 merge/toMerged/mergeWith | 16 | "needs a distinct `undefined` vs `null` representation … all-or-nothing multi-session `Type::Undefined`" | **Wrong — stale.** Distinct `undefined` **already landed** (`Constant::Undefined` + `SmeltUnknown::Undefined`, without `Type::Undefined`). The generated `merge`/`mergeWith` already emit correct `matches!(x, SmeltUnknown::Undefined)` checks. The real causes are two bounded, general bugs: an **unbound named-function-expression self-reference** and **absent-lookup producers still emitting `Null`**. |
| 10 pull/pullAt/remove | 12 | "same value-vs-reference `SmeltList` semantics as family 2(b) — mutation applied to a clone" | **Wrong.** The callees already take `&mut SmeltList<T>`. The emitter's `&mut`-list **write-back adapter exists** but is gated to fire only when the argument is itself a `&mut` param of the current function, so every ordinary caller gets `&mut <rvalue temporary>` and the mutation is discarded. A one-predicate emitter fix. |

Net effect: roughly **45 of the ~79 targeted tests are reachable by bounded,
general patches**, not by the multi-session features the earlier analysis
predicted. The genuinely large item left is family 3's rep stack.

---

# Family 5 — `merge` / `toMerged` / `mergeWith` (16 tests)

## 1. Root cause: the recorded hypothesis is stale and must be discarded

The recorded root cause is *"needs a distinct `undefined` vs `null`
representation … `undefined`/`null` collapse is `Type::None` in HIR … a
pervasive `Type::Undefined` change, explicitly documented as multi-session"*.

**That work already shipped.** Distinct `undefined` landed via the lighter
`Constant::Undefined` route (deliberately *not* `Type::Undefined`):

- `Literal::Undefined` — 18 sites across `smelt-frontend-ts`, `smelt-hir`, `smelt-mir`.
- `Constant::Undefined` — 14 sites (`smelt-mir/src/lower/{mod,stmt}.rs`,
  `smelt-codegen-rust/src/emitter/{literals,coercion,binary_ops,core}.rs`).
- `SmeltUnknown::Undefined` — 122 sites in the emitter + prelude.
- `Type::Undefined` — **0 sites**, and it is not needed.

And the generated code proves the consumers work.
`third_party/es-toolkit/dist-smelt/src/merge.rs:110` and `:169`:

```rust
_smelt_tmp_24 = matches!(target_value, SmeltUnknown::Undefined);
```

`dist-smelt/src/mergeWith.rs:86`:

```rust
_smelt_tmp_15 = matches!(merged.clone(), SmeltUnknown::Undefined);
```

`mergeWith`'s own recursion (`merge_with_975` calling itself, forwarding the
callback as `&*merge`) is also emitted correctly — `mergeWith.rs:100`, `:108`.
So neither "distinct undefined" nor "recursive closure support" is the blocker.

**Anyone inheriting the old hypothesis would open a multi-session
`Type::Undefined` project to fix two bounded bugs. Do not.**

## 1b. Root cause A (dominant, ~10 tests): named function expression self-reference is unbound

`third_party/es-toolkit/src/object/toMerged.ts` passes a **named function
expression** as the customizer and recurses through its own name:

```ts
return mergeWith(cloneDeep(target), source, function mergeRecursively(targetValue, sourceValue) {
  if (Array.isArray(sourceValue)) {
    if (Array.isArray(targetValue)) {
      return mergeWith(clone(targetValue), sourceValue, mergeRecursively);
    …
```

In `dist-smelt/src/toMerged.rs`, every one of the four `mergeRecursively`
argument positions is emitted as an **empty record**:

```rust
_smelt_tmp_7 = SmeltRecord::from([]);
_smelt_tmp_8 = SmeltUnknown::Object(SmeltObject::from_unknown_record((_smelt_tmp_7.clone()).clone()));
… merge_with_975(…, …, &*({ let smelt_source_value = _smelt_tmp_8.clone().clone(); … }))
```

The callback coercion then finds no `SmeltUnknown::Function` and no
`__smelt_call` field, so it falls through to `smelt_default_callback`, which
returns `SmeltUnknown::Null`. `mergeWith` sees `merged !== undefined` → **true**
→ writes `null` into every nested key. That is a catastrophic silent miscompile,
and it is exactly the shape of all ten `toMerged` failures (including
`should_deeply_merge_nested_objects`, `should_merge_arrays_deeply`,
`should_handle_merging_with_null_values`).

Confirmed at the emit site: `crates/smelt-frontend-ts/src/lowering/expr/operators.rs:3220`
(`function_expression_value`). It does
`let saved_locals = std::mem::take(&mut self.locals);` (`:3250`), binds each
parameter into `self.locals` (`operators.rs:3305`, `:3364` region), then resolves
captures by looking each free name up in the **outer** scope:

```rust
let Some(source_local) = saved_locals.get(name.as_str()).copied() else { … };
```

`function.id` — the function expression's own name — is **never inserted into
`self.locals`, and is not in `saved_locals` either** (it is the expression's own
binding, not an outer one). So `mergeRecursively` resolves to nothing and
degrades to a default-initialized record.

This is a general JS/TS semantics gap (ES named function expressions bind their
own name inside their body), not an es-toolkit shape.

## 1c. Root cause B (~4–6 tests): absent-lookup producers still emit `Null`

`merge`/`mergeWith` branch on `targetValue === undefined`, and the customizer
receives `targetValue`. But the **erased dynamic index read** still produces
`Null` for an absent key. `dist-smelt/src/mergeWith.rs:61` / `merge.rs:56`:

```rust
SmeltUnknown::Object(values) => values.get(&key.clone().clone()).unwrap_or(SmeltUnknown::Null),
_ => SmeltUnknown::Null,
```

Note the contrast: the **static** field read path is already correct — `clone.rs:91`
emits `match prototype { SmeltUnknown::Object(map) => smelt_get_object_field(&map, "constructor"), _ => SmeltUnknown::Undefined }`.
Only the computed/dynamic path was missed by the `c4d6b257` producer sweep.

Emit sites, all in `crates/smelt-codegen-rust/src/emitter/place.rs`:
lines **52, 393, 403, 780, 788, 791, 822, 828, 830** (plus `:148` for the
`length`-of-nullish fallback). 22 `unwrap_or(SmeltUnknown::Null)` occurrences
exist across the emitter; each needs an individual judgement — some are correct
(`JSON.parse` null, an actual `null` literal), the *absent-lookup* ones are not.

This directly explains `toMerged should_not_overwrite_existing_values_with_undefined_from_source`
and `mergeWith should_respect_null_returned_from_customizer` (which requires
`null` and `undefined` to be *distinguishable* in the customizer's return).

## 1d. Root cause C (~2 tests): closure fall-off-the-end returns `Null`

`mergeRecursively` has no `else` — falling off the end returns `undefined` in JS.
The generated closure's final arm is `SmeltUnknown::Null` (`toMerged.rs`, the
innermost `} else { SmeltUnknown::Null }`), and `smelt_default_callback` bodies
likewise return `SmeltUnknown::Null` (`toMerged.rs:8`, `pullAt_1.rs:12`,
`pullAt.rs:28`). Source: `crates/smelt-codegen-rust/src/emitter/types.rs:985`
(`Type::Unknown => Ok(self.null_value_text())`) and
`emitter/coercion.rs:1520-1522` (`null_value_text` → `"SmeltUnknown::Null"`),
plus the two hard-coded default-callback templates at `types.rs:1046` and `:1148`.

Because `mergeWith` tests `merged !== undefined`, a `Null` fall-through makes
*every* non-mergeable branch write `null`. This is the same class of bug as B and
should ship in the same commit.

## 2. Design

Preference order per `AGENTS.md`: this family needs **no new `SmeltUnknown`**. It
is a producer-correctness fix over an already-tagged dynamic value plus a
frontend scope fix.

**A. Bind the function expression's own name (frontend).**
In `function_expression_value` (`crates/smelt-frontend-ts/src/lowering/expr/operators.rs:3220`),
after the parameter locals are installed and before the body is lowered: if
`function.id` is `Some(binding)`, declare a body-local of the closure's own
function type and insert it into `self.locals` under that name, so free
references inside the body resolve to the closure itself rather than falling
through to capture resolution.

Two candidate representations, in the plan's preferred order:

1. **Self-binding local initialised from the closure value.** Emit the closure
   into a local first, then let the body reference that local. This requires the
   local to be visible *inside* the body, i.e. the existing shared-capture
   machinery (`replace_shared_capture_uses`, driven from
   `emitter/closures.rs:5`) already used for `smelt_capture_self` in
   reference-class methods. Prefer this: it reuses a proven mechanism and keeps
   the callback a concrete `Rc<dyn Fn…>` with no erasure.
2. If (1) proves circular in HIR (the closure's own value is not yet available
   when its body is lowered), fall back to treating the self-name as a
   **named-function-expression capture of an `Rc<RefCell<Option<Rc<dyn Fn…>>>>`
   cell** filled immediately after construction — still concrete, still no
   `SmeltUnknown`.

Do **not** desugar the named function expression into a hoisted top-level `fn`:
that would be a special case and would break when the body also captures outer
locals (`toMerged` does not, but `mergeWith` callers in general will).

**Module placement:** the binding itself belongs in the existing
`crates/smelt-frontend-ts/src/lowering/expr/operators.rs` next to
`function_expression_value` (that is the focused home for function-expression
lowering). If the shared-capture wiring needs more than ~40 lines, put the
self-binding helper in a new focused module
`crates/smelt-frontend-ts/src/lowering/expr/function_self_binding.rs` with a
module docstring explaining the ES named-function-expression scoping rule, per
the Rust-codegen module guidance.

**B. Flip absent-lookup producers to `Undefined`.**
In `crates/smelt-codegen-rust/src/emitter/place.rs`, change the *absent-lookup*
fallbacks (lines 52, 393, 403, 780, 788, 791, 822, 828, 830) from
`SmeltUnknown::Null` to `SmeltUnknown::Undefined`. Because these strings are
repeated across several templates, introduce a single
`fn absent_lookup_value_text(&self) -> String` next to
`null_value_text` in `emitter/coercion.rs:1520` and route all of them through
it, with a docstring stating the JS rule (*a missing property or an
out-of-bounds index reads as `undefined`, never `null`*). Centralising is also
what made avoidable erasure fall last time (`blocker-logs/estk-clone-family.md`).

Leave `Null` where `Null` is correct: JSON parse results, explicit `null`
literals, and the `_ => SmeltUnknown::Null` arm for a non-object/non-array
receiver where JS would *throw* (that arm is a separate, pre-existing deviation
— do not silently change its meaning in this commit).

**C. Flip implicit-return / default-callback values to `Undefined`.**
`emitter/types.rs:985` (`Type::Unknown => null_value_text()`) is used for more
than closure tails, so do **not** flip it wholesale. Instead flip the two
default-callback templates (`types.rs:1046`, `:1148`) and the closure/function
implicit-return path specifically. Locate the latter by the value emitted for a
JS function whose control flow reaches the end without `return`; a
`Type::Unknown` return type there is JS `undefined`, and `default_value` is the
wrong helper to serve it. Add a dedicated
`fn implicit_return_value_text(&self)` rather than widening `default_value`,
because `default_value` also serves genuine Rust `Default` positions where
`Null` may be load-bearing.

## 3. Blast radius

- **Remeda (1789/0): real risk, medium.** The `wip/distinct-undefined-grind`
  history in project memory is explicit that *"each producer added without the
  FULL reconciliation breaks more"* — literal-only production measured **net −8**,
  and adding missing-access + nullish-coalesce measured **net −17**. B and C here
  are exactly "more producers". Since then the reconciliation *did* land
  (`c4d6b257`: `defaultTo`/`isEmptyish`/`??`/loose `==null`/`prop`), so the
  library side should now absorb them — but that is an inference, not a
  measurement. **Gate B and C behind a full remeda run, separately from A.**
- **SmeltUnknown ratchet: neutral.** `SmeltUnknown::Null` → `SmeltUnknown::Undefined`
  is token-for-token identical per line, and `classify_line`
  (`crates/smelt-transpiler/src/unknown_report.rs:583`) counts per line, not per
  variant. Centralising into `absent_lookup_value_text` should make avoidable
  erasure **fall** (fewer inline tokens at ~9 templates × many sites), which
  requires a same-commit re-snapshot of
  `blocker-logs/smelt-unknown-baseline-es-toolkit.json`.
- **MIR ownership passes: unaffected.** Neither A nor B/C changes types,
  borrows, or move points. A adds one local + one capture; that local is a
  closure handle (`Rc`), which the move-on-last-use pass already handles for
  `smelt_capture_self`.
- **Goldens:** A changes the emitted text for every named function expression in
  the fixture corpus. Expect golden churn in `smelt-frontend-ts` and
  `smelt-codegen-rust` tests. This is the main mechanical cost of A.
- **Watch for over-firing:** binding the self-name must not shadow an outer
  binding of the same name *after* the closure (JS scopes the self-name to the
  body only). A test for `const f = function g(){…}; g()` failing at the outer
  scope is the right regression guard.

## 4. Effort

- A (self-binding): **bounded patch → small multi-day**, mostly golden churn.
  Moves ~10 tests (`toMerged`), plus latent wins wherever a named function
  expression recurses.
- B (absent-lookup producers): **bounded patch**, but must be report-gated
  against remeda. Moves ~4–6 tests in this family and is a prerequisite for
  parts of families 2/3/11.
- C (implicit return / default callback): **bounded patch**. Moves ~2 here,
  more in `cloneDeepWith` (family 2) where the customizer's implicit
  `undefined` is the whole contract.
- Whole family: **16 tests**, realistically 14–16 with A+B+C. **No multi-session
  feature is required.** This is the single biggest scope correction in the plan.

---

# Family 10 — `pull` / `pullAt` / `remove` in-place mutation (12 tests)

Planned here out of order because it is the cheapest confirmed win and because
the recorded hypothesis wrongly couples it to family 2(b).

## 1. Root cause: the recorded hypothesis is wrong

Recorded: *"the mutation is applied to a clone, so the caller's array is
unchanged … shares its lever with family 2(b) — `SmeltList` reference
semantics"*.

The callees already take `&mut`. `dist-smelt/src/pull_1.rs:7`:

```rust
pub(crate) fn pull_127(mut arr: &mut SmeltList<SmeltUnknown>, values_to_remove: SmeltList<SmeltUnknown>) -> SmeltList<SmeltUnknown>
```

`remove_1.rs:7` and `pullAt_1.rs:7` likewise. The MIR mutating-parameter
analysis is doing its job.

The defect is at the **call site**. `dist-smelt/src/pull_spec.rs:16`:

```rust
pull_127(&mut { let smelt_l: SmeltList<_> = (array.clone()).clone().into();
               SmeltList::with_id(smelt_l.id(), smelt_l.into_iter()
                 .map(|value| SmeltUnknown::Number(value as f64)).collect::<Vec<_>>()) }, …)
```

The caller holds `array: SmeltList<f64>`; the callee wants
`&mut SmeltList<SmeltUnknown>`. The emitter builds a re-typed **rvalue
temporary** and takes `&mut` of *that*. The temporary is dropped at the end of
the statement and `array` is never written back. Same shape at
`remove_spec.rs:23`, `:55`, `:86` and throughout `pullAt_spec.rs`.

The emitter already knows how to do this correctly. `dist-smelt/src/pull.rs:8`
(the compat wrapper) shows the working adapter:

```rust
let mut smelt_mut_arg_0: SmeltList<SmeltUnknown> = (*arr).clone().into_iter()
    .map(IntoSmeltUnknown::into_smelt_unknown).collect::<SmeltList<SmeltUnknown>>();
let smelt_mut_call_result = pull_127(&mut smelt_mut_arg_0, values_to_remove.clone());
*arr = smelt_mut_arg_0.into_iter().map(|e| <T as SmeltFromUnknown>::smelt_from_unknown(e)).collect::<SmeltList<_>>();
```

The adapter is `mut_list_adapter_arg` in
`crates/smelt-codegen-rust/src/emitter/call.rs:1428`, rendered by the block at
`call.rs:1333-1387`. It is gated off at **`call.rs:1449-1454`**:

```rust
// The argument must itself be a `&mut` list parameter of the current
// function; only then does the emitted argument text reborrow through a
// reference whose element type must match invariantly.
if self.function.id.0 == u32::MAX
    || !self.function.params.contains(&local)
    || !self.parameter_needs_mutable_reference(local)
{
    return Ok(None);
}
```

`array` in a test body is an ordinary local, not a param, so
`!self.function.params.contains(&local)` rejects and the broken fallback
(`call.rs:1367`, `value_at_type`) runs. `FuncId(u32::MAX)` is the synthetic
closure pseudo-function (`emitter/closures.rs:426`), so closure bodies are
excluded too.

**This is one over-narrow predicate, not a container-semantics feature.**

## 2. Design

Widen the gate at `call.rs:1449` from "is a `&mut` list param of the current
function" to "**is any assignable list place whose rendered element type differs
from the callee's rendered parameter element type**":

- Keep the `arg_element_text == param_element_text` early-out at `call.rs:1478`
  — when the element types already match, no adapter is needed and the direct
  `&mut place` is correct and cheaper.
- Accept `Place::Local(local)` for a plain local, and emit
  `let mut smelt_mut_arg_N: <callee ty> = <place>.clone().into_iter()…` /
  `<place> = smelt_mut_arg_N.into_iter()…` (drop the `*` deref that the
  current template hard-codes at `call.rs:1358`/`:1362` — it is only correct for
  a `&mut` param; derive it from whether the place is a reference).
- Keep the `FuncId(u32::MAX)` exclusion for now unless a closure-body case is
  demonstrated; widening two things at once makes the remeda delta unreadable.
- Reject non-place operands (`Operand::Constant`, temporaries) — writing back to
  a temporary is meaningless, and the current `matches!` at `call.rs:1442` already
  restricts to `Place::Local`.

`Place::Field` / `Place::Index` bases (`obj.items` passed to `pull`) should be
handled in a **follow-up**, not this commit: the write-back target then needs the
list-alias machinery in `emitter/list_mutation.rs:145` (`list_alias_origin`,
which already models "a list local copied out of a mutable JavaScript
property"). Note the emitted text-rewrite helper
`crates/smelt-codegen-rust/src/emitter/rendered_text_rewrite.rs` and the
adapter-block scanner at `emitter/core.rs:4288-4292` (which keys on
`"let mut smelt_mut_arg_"`) both need to keep working — the scanner's marker is
unchanged by this design.

New code belongs in `emitter/call.rs` alongside the existing adapter (it is the
focused home), with the widened predicate factored into a documented
`fn mut_list_adapter_place(&self, arg: &Operand) -> Option<Place>` so the
"which places can be written back" rule is stated in one place.

**No `SmeltUnknown` change.** The adapter is already an explicit boundary
adapter using `IntoSmeltUnknown` / `SmeltFromUnknown`, exactly the shape
`AGENTS.md` asks for.

## 3. Blast radius

- **Remeda: low-to-moderate risk, but real.** This changes the emission of
  *every* call passing a list to a mutating parameter with a differing element
  type. Remeda has in-place-mutation functions; a write-back that is *newly*
  correct could still change observable ordering if the same place is both an
  argument and read later in the same statement. The three-address MIR form
  makes that unlikely (`blocker-logs/reference-classes.md` borrow-discipline
  notes), but it is the failure mode to look for.
- **Ratchet: likely falls.** The adapter's lines contain `IntoSmeltUnknown` /
  `into_smelt_unknown`, which are `BOUNDARY_MARKERS` in
  `unknown_report.rs:612-617`, so adapter lines classify as
  **legitimate-boundary** (never blocks). The lines they *replace* (e.g.
  `pull_spec.rs:16`, which carries bare `SmeltUnknown::Number(...)` with no
  boundary marker) classify as **avoidable**. Expect avoidable to fall and
  legitimate to rise → re-snapshot in the same commit.
- **MIR ownership passes: unaffected in the compiler, but interacting.** The
  adapter clones the place, so the borrow-read-only-collection-params pass
  cannot see through it; nothing regresses, but a place that move-on-last-use
  previously moved into the call will now be cloned then written back. Watch for
  new `clippy::redundant_clone` on the generated crate — cosmetic, not a
  blocker.
- **Goldens:** every fixture that passes a list to a mutating param with an
  element-type mismatch changes. `crates/smelt-codegen-rust/src/tests/part_7_tests.rs:7673`
  already asserts on `smelt_mut_arg_0` and is the natural place to extend.

## 4. Effort

**Bounded patch.** Moves **~12 tests** (`pullAt` 6, `pull` 3, `remove` 3), plus
whatever the same defect is silently costing in the long tail (family 11 has
several `toEqual` failures in mutating helpers). Highest tests-per-line-changed
of anything in this plan.

---

# Family 2 — `clone` / `cloneDeep` / `cloneDeepWith` (38 tests)

`cloneDeep_spec` 17 + `clone_spec` 15 + `cloneDeepWith_spec` 6 = 38.

## 1(a). Root cause: prototype reflection, not "one exotic rep at a time"

The recorded hypothesis lists ~12 missing host reps and calls it *"stdlib
surface, one rep at a time — not a bounded codegen patch"*. Two of those claims
need correcting, and one large general cause was missed.

**Corrected — the stale claim.** `blocker-logs/estk-clone-family.md` said
`clone_spec` failures share a defect where the merged
`Array || isTypedArray || ArrayBuffer` branch *"treats a plain array as a
buffer/typed-array → string `slice`, returning a String for `[1,2,3]`"*. That is
no longer true: `dist-smelt/src/clone.rs:73` shows the slice emitter dispatching
correctly on `SmeltUnknown::Array(values) => … SmeltUnknown::Array(SmeltArray::with_id(…))`.
Anyone starting from that report would chase a fixed bug.

**Missed — the dominant cause.** `clone.ts` is *prototype-driven*:

```ts
const prototype = Object.getPrototypeOf(obj);
if (prototype == null) return Object.assign(Object.create(prototype), obj);
const Constructor = prototype.constructor;
…
if (typeof obj === 'object') { const newObject = Object.create(prototype); return Object.assign(newObject, obj); }
```

Smelt models prototypes as **opaque string sentinels**
(`smelt_prototype_sentinel`, generated at `dist-smelt/src/main.rs` in the
prelude; emitted from `crates/smelt-codegen-rust/src/lib.rs`). For a plain
object it returns `SmeltUnknown::String("__smelt_proto:object")`. Two
consequences, both visible in the generated tail of `clone.rs`:

1. `Object.create(prototype)` lowers to the **identity of `prototype`**:
   `clone.rs:246` — `new_object = prototype.clone();`
2. `Object.assign(newObject, obj)` then coerces that *string* to a record, and
   the string→record coercion **char-enumerates**: `clone.rs:247` —
   `SmeltUnknown::String(value) => value.chars().enumerate().map(|(index, ch)| (index.to_string(), …))`.

So `clone({ a: 1, b: 'es-toolkit', c: [1,2,3] })` returns an object with ~21
numeric char keys plus the real fields. That single defect accounts for
`clone_spec::should_clone_objects`, `should_shallow_clone_nested_objects`,
`should_clone_objects_with_a_null_prototype`, and blocks the `.constructor`
lookup that `should_clone_maps`, `should_clone_regular_expressions`,
`should_clone_error`, `should_clone_custom_error`, `should_clone_custom_classes`
and `should_clone_data_views` all depend on (`clone.rs:91`:
`constructor = match prototype { SmeltUnknown::Object(map) => smelt_get_object_field(&map, "constructor"), _ => SmeltUnknown::Undefined }`
— a `String` sentinel takes the `_` arm and yields `Undefined`, after which
every `new Constructor(...)` call at `clone.rs:108/114/123/127/146` falls to
`else { SmeltUnknown::Null }`).

`smelt_reflected_prototype(kind)` already returns a real object carrying a
`constructor` function — but only for *marked* kinds (date/map/set/regexp/
dataview/error/file/number/boolean). Plain objects, arrays, promises and class
instances get bare string sentinels. **The general fix is to close that gap, not
to add reps one at a time.**

## 1(b). Root cause: erased array reference identity — right diagnosis, wrong container

The recorded lever is *"`SmeltList` lacks the `Rc<RefCell<..>>` reference
semantics that `SmeltJsMap` already got"*. The diagnosis of the *symptom* is
correct; the proposed container is wrong.

Reading the generated prelude (`dist-smelt/src/main.rs:2330`, `:2628`, `:2691`):

```rust
pub struct SmeltList<T> { id: usize, values: Vec<T> }
impl<T: Clone> Clone for SmeltList<T> { fn clone(&self) -> Self { Self { id: self.id, values: self.values.clone() } } }

pub struct SmeltObject { id: usize,
    values: Rc<RefCell<HashMap<String, SmeltUnknown>>>, order: Rc<RefCell<Vec<String>>> }
impl Clone for SmeltObject { fn clone(&self) -> Self { Self { id: self.id, values: self.values.clone(), order: self.order.clone() } } }

pub struct SmeltArray { id: usize, values: Vec<SmeltUnknown> }
impl Clone for SmeltArray { fn clone(&self) -> Self { Self { id: self.id, values: self.values.clone() } } }
```

So **erased objects already have reference semantics** (shared `Rc`, shared
`id`) — which is why `merge`'s in-place object mutation works. But `SmeltArray`
and `SmeltList` are in a **worse-than-either state**: `Clone` *preserves the
`id`* (so `===` and `Object.is` compare equal) while *copying the `Vec`* (so
mutation is not shared). Two handles that are `===` equal but disagree about
contents. That inconsistency, not plain value semantics, is what
`cloneDeep_spec::should_deep_clone_nested_objects` trips over:

```ts
const nestedObj = { a: [1, 2, 3], b: { c: 'es-toolkit' }, d: new Date() };
const clonedNestedObj = cloneDeep(nestedObj);
nestedObj.a[2] = 4;                                    // lost if `.a` reads a copy
expect(clonedNestedObj.a[2]).not.toEqual(nestedObj.a[2]);
```

**Lift `SmeltArray`, not `SmeltList`.** The asymmetry is decisive:

| | emitter surface | generated-code surface | `Deref`/`Index` |
|---|---:|---|---|
| `SmeltArray` (erased) | **30 mentions** across `emitter/{core,list,coercion}.rs` + `lib.rs` | only `SmeltArray::with_id` (1112×) and `SmeltArray::new` (1×) | `Deref<Target=[SmeltUnknown]>` only — **immutable slice**, no `DerefMut`, no `Index`, no `splice` |
| `SmeltList<T>` (typed) | ~70 mentions, ~1800 method-emitting sites | **~9,500** `Vec`-method calls in one crate: `.get` 4772, `.len` 1737, `.iter` 1086, `.push` 640, `.insert` 436, `.is_empty` 396, plus `.splice`, `.resize`, `.drain`, `.sort_by`, `arr[i] = v` | `Deref` **and** `DerefMut` **and** `Index`/`IndexMut` |

`SmeltJsMap` was cheap to lift precisely because it had a hand-written ~8-method
surface returning owned values and **no `Deref`**. `SmeltList` is the opposite.
Concretely, `Rc<RefCell<Vec<T>>>` makes these *impossible*, not merely tedious:

- `Deref`/`DerefMut` cannot return `&Vec<T>` / `&mut Vec<T>` through a
  `RefCell` guard (the guard is a temporary). All ~9,500 sites lose their
  method resolution at once.
- `.get(i)` must become `Option<T>`, but generated code writes
  `.get(i).cloned().unwrap_or(…)` — `Option<T>::cloned` does not exist. 4,772 sites.
- `.iter()` must return an owning iterator, but generated code writes
  `.iter().cloned()` — `Iterator<Item = T>` has no `.cloned()`. 1,086 sites.
- `Index`/`IndexMut` (`arr[i] = v`, `pull_1.rs:51`) cannot be implemented at all.
- `.splice(range, vec)` returns a draining iterator borrowing the `Vec`; it
  cannot escape a `RefCell` guard.

**Recommendation: do not lift `SmeltList` in this campaign.** Family 10 is
solved by the write-back adapter (above), which is where typed in-place mutation
belongs — Rust ownership + an explicit boundary adapter, i.e. *concrete types
first*. Lifting `SmeltArray` gives JS reference semantics exactly where the
language actually requires them: behind the erased `SmeltUnknown::Array`
boundary, mirroring `SmeltObject`.

## 2. Design

**2(a) — general prototype reflection.** Replace the plain-object/array/promise
string sentinels with real prototype **objects** carrying a `constructor`, so
`prototype.constructor` and `new Constructor(...)` work through the same
`smelt_reflected_prototype` path that marked kinds already use:

- Extend `smelt_reflected_marker_kind` / `smelt_reflected_prototype` with
  `"object"`, `"array"`, `"promise"` and `"class"` kinds. Keep the existing
  chain-termination contract documented in the prelude (each sentinel must
  advance one link toward `null`, or the `while (Object.getPrototypeOf(p) !== null)`
  walk never terminates) — the new prototype objects must therefore themselves
  report `Object.prototype`, whose prototype is `null`.
- Make `Object.create(proto)` produce a **fresh empty object** that remembers
  `proto`, instead of returning `proto` (the current `new_object = prototype.clone()`).
  Store the link in a reserved hidden key (the codebase's established idiom —
  `__smelt_class`, `__smelt_error`, `__smelt_regexp` …), e.g.
  `__smelt_proto`, and teach `smelt_prototype_sentinel` and
  `smelt_is_for_in_object_key` about it so it stays invisible to `for…in`,
  `Object.keys` and `toEqual`. `Object.create(null)` must produce a
  prototype-less object, which is what `should_clone_objects_with_a_null_prototype`
  and `isEqualWith should_treat_objects_created_by_object_create_null_like_plain_objects`
  both require.
- For a plain object the synthesized `constructor` must behave like `Object`
  (`new Object(x)` → `smelt_fresh_identity(x)`), which `smelt_reflected_construct`
  already does in its `else` arm.

**Module placement.** The prelude is emitted as string literals from
`crates/smelt-codegen-rust/src/lib.rs` (4310 lines, already carrying
`SmeltList`/`SmeltObject`/`SmeltArray`/`SmeltUnknown`). Per the Rust-codegen
guidance ("keep Rust source emission helpers in separate modules"), put the
prototype-reflection prelude in a **new focused module**
`crates/smelt-codegen-rust/src/prelude/prototype.rs` (or
`src/prelude_prototype.rs` if no `prelude/` dir exists yet), with a module
docstring stating the sentinel/termination invariant. Do not grow `lib.rs`.

**Exotic reps (the residual ~8–10 tests).** ArrayBuffer / SharedArrayBuffer /
typed arrays / Buffer / DataView / Blob / File / boxed `Boolean|Number|String` /
`arguments`. Markers for all of these **already exist** in
`smelt_object_to_string_tag` and `smelt_object_has_host_marker`
(`__smelt_arraybuffer`, `__smelt_buffer`, `__smelt_dataview`, `__smelt_blob`,
`__smelt_file`, `__smelt_number`, `__smelt_boolean`, `__smelt_string`) — what is
missing is *byte storage and `.slice()`/`.valueOf()` behaviour* on them, not the
tag. Design them as **concrete host records with a typed byte payload**
(a `SmeltUnknown::Array` of numbers behind the marker key, or a dedicated
`Rc<RefCell<Vec<u8>>>` field on a small prelude struct), and expose
`slice`/`byteLength`/`valueOf` as prelude helpers. Explicitly **not**
`SmeltUnknown`-widening: these are host objects with known shape.

Take them **one marker per commit**, each with its own generated-suite delta.
This is the part of family 2 that genuinely is incremental stdlib work.

**2(b) — lift `SmeltArray` to shared storage.**

```rust
pub struct SmeltArray { id: usize, values: Rc<RefCell<Vec<SmeltUnknown>>> }
impl Clone for SmeltArray { fn clone(&self) -> Self { Self { id: self.id, values: Rc::clone(&self.values) } } }
```

Then:
- Replace `Deref<Target=[SmeltUnknown]>` with inherent owned-returning methods
  (`len`, `get(i) -> Option<SmeltUnknown>`, `iter() -> vec::IntoIter<…>`,
  `push`, `set_index`, `into_vec`), exactly the `SmeltJsMap` shape. There are
  only ~30 emitter mentions and one generated constructor, so the sweep is
  tractable — but the *prelude helpers* in `lib.rs` that touch `array.values`
  directly (`smelt_fresh_identity`, `smelt_structured_clone`,
  `smelt_index_assign`, the erased-iterable coercion, `From<SmeltList<_>>`)
  must all be updated, and `smelt_fresh_identity` / `fresh_copy` become the
  *only* places that create a new `Rc` — that is what preserves
  `[...a] !== a` while `a === a`.
- `From<SmeltList<T>> for SmeltArray` (`main.rs:2760`) currently moves
  `list.values`; it becomes the erase boundary that wraps a fresh `Rc`. The
  `smelt_list_identity` thread-local (which keys erased-array ids on a source
  `Vec`'s address, with a documented empty-`Vec` collision caveat) can likely be
  **retired** once erased arrays share storage — a nice simplification, but
  verify before removing.

Sequence 2(b) **after** family 10, not before: family 10's write-back adapter is
the thing that keeps typed lists correct, and doing both at once makes any
remeda delta impossible to attribute.

## 3. Blast radius

- **`SmeltArray` → `Rc<RefCell<..>>` is the single riskiest item in this plan.**
  Every erased array in every generated crate changes aliasing semantics. Two
  specific hazards:
  1. **Borrow panics at runtime.** `RefCell` turns an aliasing bug into a
     `BorrowMutError` panic instead of a silent wrong answer. Any prelude helper
     that holds a `borrow()` across a call that re-enters the same array will
     panic. `smelt_structured_clone` recurses over `array.into_vec()` and
     `smelt_index_assign` takes `&mut SmeltUnknown` — both need auditing for
     guard lifetime. `blocker-logs/reference-classes.md` documents the same
     hazard class for reference classes and the mitigation (three-address MIR
     means a guard never spans a re-entrant call *within one statement*) — that
     argument must be re-checked for the prelude helpers, which are hand-written
     Rust and not MIR-generated.
  2. **Cyclic `Rc` leaks.** Circular structures (which `isEqualWith`'s
     circular-reference tests deliberately build) become `Rc` cycles that never
     drop. `SmeltObject` already has this exposure, so it is not a new class of
     problem, but arrays make it common. Leaks do not fail tests; note and move on.
- **Remeda: high risk.** Remeda's memory notes record an entire cluster
  (`plan-list-identity-2a`, `remeda-after-list-identity-*`, eight
  `remeda-smeltlist-A*` reports) built on the *current* `SmeltList`/`SmeltArray`
  identity model. Changing erased-array aliasing can plausibly move any of them.
  Treat a full remeda run as a hard gate, and expect at least one round of
  follow-up.
- **`Type::Undefined`: not proposed, so its blast radius is moot.** Recorded for
  the record: it would touch ~190 `Type::None` match sites across three crates
  and, per project memory, cannot land net-positive incrementally. The
  `Constant::Undefined` route already shipped and made it unnecessary.
- **Ratchet:** prelude changes classify as `RuntimePrelude`
  (`unknown_report.rs:584`) and never block. The prototype work adds
  `SmeltObject`/`SmeltUnknown` constructions **in the prelude**, so avoidable
  should be flat. But `Object.create` gaining a hidden-key write may add
  *non-prelude* lines at call sites — measure, do not assume.
- **MIR ownership passes:** `SmeltArray` lives only inside `SmeltUnknown`, which
  the borrow-read-only-collection-params pass already treats as opaque. Expected
  unaffected — **unverified without a build.**

## 4. Effort

- 2(a) prototype reflection: **multi-day feature**, one coherent change. Moves
  an estimated **10–14** tests (most of `clone_spec`, several `cloneDeep_spec`),
  and is a hard prerequisite for family 3's `.constructor` comparison.
- 2(a) exotic reps: **incremental, ~1 commit per marker**, 8–12 markers.
  Moves **~8–10** tests. This is the only part of the original "not a bounded
  patch" characterisation that survives scrutiny.
- 2(b) `SmeltArray` lift: **multi-day, high-risk feature.** Moves **~3** tests
  in family 2 directly. Its value is unlocking family 3's circular-reference and
  Map-snapshot cases, not its own test count. **It is *not* the shared lever with
  family 10** — that was the recorded hypothesis and it is wrong.
- 2(c) family 5's fix C (closure implicit return → `Undefined`) also moves
  `cloneDeepWith`, whose whole contract is "customizer returning `undefined`
  means fall through". Expect **2–4** of the 6 `cloneDeepWith` failures from
  family 5's work alone, before any family-2 work starts.

---

# Family 3 — `isEqualWith` / `isEqual` (25 tests)

`isEqualWith_spec` 23 + `isEqual_spec` 2.

## 1. Root cause: confirmed, with a sharper dependency

Recorded: *"stacked value-identity semantics: `Map` deep-equality and Map
self-reference snapshots, boxed-primitive wrappers, typed-array reps, `Buffer`.
Depends on family 2's reps … mostly general, but a stack — no single lever."*

**Confirmed.** `src/predicate/isEqualWith.ts` is a tag dispatcher:

```ts
let aTag = getTag(a); let bTag = getTag(b);
if (aTag === argumentsTag) aTag = objectTag;
if (aTag !== bTag) return false;
switch (aTag) {
  case stringTag:  return a.toString() === b.toString();
  case numberTag:  { const x = a.valueOf(); const y = b.valueOf(); return eq(x, y); }
  case booleanTag: case dateTag: case symbolTag: return Object.is(a.valueOf(), b.valueOf());
  …
  case arrayBufferTag: return areObjectsEqual(new Uint8Array(a), new Uint8Array(b), …);
  …  areObjectsEqual(a.constructor, b.constructor, …)
```

The tag side is largely already served: `smelt_object_to_string_tag` in the
prelude covers Date/RegExp/Error/Map/Set/ArrayBuffer/SharedArrayBuffer/Buffer/
DataView/WeakMap/WeakSet/File/Blob/DOMException/all Intl/boxed
Number|Boolean|String|Symbol. What is missing is everything the switch *arms*
need:

1. **`.valueOf()` on boxed primitives** — required by the `numberTag`,
   `booleanTag`, `dateTag`, `symbolTag`, `stringTag` arms. Family 2(a)'s boxed
   reps are the prerequisite.
2. **`a.constructor` comparison** (`isEqualWith.ts:292`) — **blocked on family
   2(a)'s prototype reflection**, and this is the sharper dependency the recorded
   hypothesis missed. It is *not* blocked on 2(b).
3. **Typed-array / ArrayBuffer / Buffer byte views** — `new Uint8Array(a)` over
   an ArrayBuffer. Blocked on family 2(a)'s byte-payload design.
4. **The `stack: Map<any, any>` recursion guard** for circular references
   (9 of the 23 failures are circular-reference tests). `SmeltJsMap` already has
   `Rc<RefCell<Vec<(K,V)>>>` shared storage and `SmeltJsKeyEq` lookup, and
   `js_strict_eq` on arrays/objects compares by `id` (`main.rs:2620`), so
   `stack.set(a, b)` / `stack.get(a)` should already find entries by identity.
   Whether the *snapshot* semantics break (`SmeltArray::clone` copies the `Vec`
   while keeping the `id`) is the open question — see below.
5. **Symbol-keyed property comparison** — the prelude already filters
   `__smelt_symbol:`-prefixed keys out of `for…in`
   (`mergeWith.rs:30`: `!key.starts_with("__smelt_symbol:")`), so symbol keys
   exist as a rep but are invisible to the object walk that `isEqualWith` needs.
   This is a small, self-contained general fix
   (`should_compare_symbol_properties_when_customizer_returns_undefined`).
6. **`arguments` objects** — the tag-rewrite (`argumentsTag` → `objectTag`)
   needs an `arguments` rep to exist at all. Shared with **family 6** (erased
   rest/`arguments` spread, 10 tests), which is a separate cheap family.

**There is genuinely no single lever here.** The recorded characterisation is
right, and after families 2(a) and 5 land, the residue should be re-clustered
before further planning — a good fraction of these 25 may simply fall out.

## 2. Design

Nothing here should introduce `SmeltUnknown`. Every item is a **concrete host
rep plus a prelude helper**:

- `.valueOf()` — a prelude `smelt_value_of(&SmeltUnknown) -> SmeltUnknown` that
  unwraps `__smelt_number` / `__smelt_boolean` / `__smelt_string` /
  `__smelt_symbol` / `__smelt_date` payloads and is the identity elsewhere.
  Place it in the same new `prelude/prototype.rs` module as the reflection work
  (both are "host object introspection") or a sibling
  `prelude/host_boxed.rs` if that module grows past ~300 lines.
- Byte views — one concrete payload representation shared by ArrayBuffer,
  SharedArrayBuffer, DataView, Buffer and the typed arrays, so
  `new Uint8Array(buf)` is a view constructor over it rather than a new rep.
  Design this **once, in family 2(a)**, and let family 3 consume it. Doing it
  per-tag is how this family becomes a swamp.
- Symbol-keyed walk — thread a "include symbol keys" flag through the
  object-entries helper rather than adding a second helper, so `Object.keys`
  (excludes) and `isEqualWith`'s walk (includes) share one implementation.

**Order the arms by test count, not by source order**, and land one arm per
commit with a suite delta.

## 3. Blast radius

- **Lowest-risk family of the three.** Almost all of it is *additive* prelude
  surface (new helpers, new marker payloads) rather than changed semantics for
  existing values. `.valueOf()`, byte views and the symbol-key flag do not
  change how any currently-passing value behaves.
- **The exception is the symbol-key walk**: if the "include symbols" flag leaks
  into `Object.keys` / `for…in` / `toEqual`, it will regress broadly. Gate it
  behind an explicit parameter with no default change, and add a fixture
  asserting `Object.keys` still excludes symbol keys.
- **Remeda:** low risk for the additive parts; `isDeepEqual` is remeda's own
  equality function and shares the compare helpers, so run the full guard on the
  `.valueOf()` and symbol-walk commits specifically.
- **Ratchet:** prelude-only for the helpers → `RuntimePrelude`, never blocks.
- **MIR ownership passes:** unaffected (no type or borrow changes).

## 4. Effort

**Multi-day, and genuinely a stack** — but the stack is *shallower than
recorded* once family 2(a) lands, because `.constructor` and the byte payload
come for free from it. Honest split:

- Blocked-on-2(a) arms (`.constructor`, boxed `.valueOf()`, ArrayBuffer/Buffer):
  **~10–12 tests**, near-zero marginal cost once 2(a) exists.
- Symbol-keyed properties: **bounded patch**, 1–2 tests.
- `arguments` rep: **bounded**, shared with family 6 (which is worth ~10 tests
  on its own — cheaper to do there first).
- Map deep-equality + circular-reference snapshots: **~9 tests**, unknown cost
  until 2(b) lands and the failures are re-measured. Do not plan these until
  then.

---

# Recommended sequence

Ordered to unlock the most tests earliest and to keep every remeda delta
attributable to one change. Counts are estimates from the failing-test inventory
in `blocker-logs/estoolkit-runtime-current.md`, not measurements.

| # | Step | Kind | Tests | Depends on |
|---|---|---|---:|---|
| 0 | *(family 7, exception-payload ABI — in flight)* | — | ~24 | — |
| 1 | **Family 10: widen the `&mut`-list write-back gate** (`emitter/call.rs:1449`) | bounded patch | **~12** | none |
| 2 | **Family 5A: bind named-function-expression self-name** (`lowering/expr/operators.rs:3220`) | bounded → small feature | **~10** | none |
| 3 | **Family 5B+C: absent-lookup and implicit-return producers → `Undefined`** (`emitter/place.rs` ×9, `emitter/types.rs:1046/1148`) | bounded patch | **~6** here + **~2–4** in `cloneDeepWith` | none (gate on remeda) |
| 4 | *(optional detour)* **Family 6: erased `arguments`/rest rep** | bounded | ~10, plus unblocks 3's `arguments` arm | none |
| 5 | **Family 2(a)-i: prototype reflection** (`Object.create`, `prototype.constructor`, new prelude module) | multi-day feature | **~10–14** | none |
| 6 | **Family 2(a)-ii: host byte-payload + boxed-primitive reps**, one marker per commit | incremental, 8–12 commits | **~8–10** | 5 |
| 7 | **Family 3: the arms unblocked by 5+6** (`.constructor`, `.valueOf()`, ArrayBuffer/Buffer, symbol keys) | multi-day | **~12–14** | 5, 6 |
| 8 | **Family 2(b): lift `SmeltArray` to `Rc<RefCell<Vec<..>>>`** | multi-day, high risk | **~3** direct | 1 (must land first) |
| 9 | **Family 3 residue: Map deep-equality + circular refs** — re-measure before planning | unknown | ~9 | 8 |

Cumulative through step 3: **~30 tests for three bounded patches** — this is the
plan's main finding. Steps 1–3 are independent of each other and of family 7, so
they can be dispatched in parallel (subject to the two-concurrent-builder cap).

**Single recommendation for what to implement next after family 7 lands:**
**step 1 — widen the `&mut`-list write-back gate at
`crates/smelt-codegen-rust/src/emitter/call.rs:1449-1454`.** It is one
predicate, the correct adapter already exists and is already emitted correctly
elsewhere in the same crate (`dist-smelt/src/pull.rs:8` proves it), it moves ~12
tests, it should *lower* avoidable erasure, and it removes family 10's false
dependency on the `SmeltList` container change — which frees family 2(b) to be
deferred to last, where its risk belongs.

---

# Open questions for the implementer

Everything below could not be settled by reading. **Nothing in this plan was
compiled.**

1. **Does step 3 (Null→Undefined producers) regress remeda?** Project memory
   records `−8` and `−17` net regressions from earlier partial producer batches,
   before the `c4d6b257` reconciliation landed. Whether that reconciliation now
   absorbs the dynamic-index-read producer is the single most important
   unverified assumption in this plan. **Run the full remeda guard on step 3
   alone, before combining it with anything else.**
2. **Which of the 22 `unwrap_or(SmeltUnknown::Null)` emitter sites are
   absent-lookup and which are legitimately `null`?** I classified the nine in
   `place.rs` by reading their surrounding templates; the remaining thirteen
   (`coercion.rs`, `call_runtime.rs`, `strings.rs`, `list_query.rs`,
   `optional_access.rs`) need individual review. Do not bulk-replace.
3. **Is the closure implicit-return value served by
   `default_value(Type::Unknown)` (`types.rs:985`) or by a separate path?** I
   could not pin the exact emit site for "control flow reaches the end of a JS
   function body". `types.rs:985` is shared with genuine Rust `Default`
   positions, so flipping it wholesale is probably wrong — find the specific
   path first.
4. **Can the function-expression self-name be bound without a circular HIR
   dependency?** The closure's own value may not exist when its body is lowered.
   Design option (1) assumes the `smelt_capture_self` shared-capture machinery
   (`emitter/closures.rs:5`, `replace_shared_capture_uses`) can carry it; option
   (2) is the fallback. Unverified.
   - Related known bug to avoid tripping: `replace_shared_capture_uses`
     **rewrites captured identifiers inside Rust string literals** (documented in
     `estoolkit-runtime-current.md`, "Notes for the next agent"). A capture named
     `mergeRecursively` is unlikely to collide, but a capture named e.g. `key`
     would corrupt any program string containing "key". Consider fixing that
     first — it is bounded and general.
5. **Does the widened write-back adapter (step 1) change move-on-last-use
   behaviour for the affected places?** The adapter clones then writes back where
   the pass previously moved. Expected benign; needs a build to confirm no new
   `E0382`/`E0505`.
6. **Does `pull_at_574` (`dist-smelt/src/pullAt.rs:7`) need more than step 1?**
   It takes `array: SmeltList<SmeltUnknown>` **by value**, not `&mut` — so the
   mutating-parameter analysis did not mark it, unlike `pull_127`/`remove_130`.
   Either the compat `pullAt` genuinely does not mutate its argument, or the
   analysis missed it. Six tests hinge on this; check `src/compat/array/pullAt.ts`
   against the analysis before assuming step 1 covers `pullAt_spec`.
7. **Is `nestedObj.a[2] = 4` (the `cloneDeep` nested-reference test) handled by
   the existing list-alias machinery?** `emitter/list_mutation.rs:145`
   (`list_alias_origin`) explicitly models "a list local copied out of a mutable
   JavaScript property", which may already cover a direct
   `record.field[index] = v` place assignment without needing 2(b). If it does,
   family 2(b)'s direct test count drops from ~3 to ~1 and it should be deferred
   further. **Check this before starting 2(b).**
8. **How many `RefCell` borrow-panic hazards does the `SmeltArray` lift create in
   the hand-written prelude?** `smelt_structured_clone`, `smelt_fresh_identity`,
   `smelt_index_assign` and the erased-iterable coercion all touch
   `array.values` directly. The three-address-MIR argument that protects
   *generated* code does not protect the prelude, which is hand-written.
9. **Can `smelt_list_identity` (the `Vec`-address-keyed erased-array id
   thread-local, with its documented empty-`Vec` collision caveat) be retired
   after the `SmeltArray` lift?** It exists to make repeated erasures of one
   source list compare `===`; shared storage may subsume it. Verify rather than
   assume.
10. **Exact ratchet deltas.** I predict avoidable *falls* for steps 1 and 3
    (boundary markers `IntoSmeltUnknown`/`into_smelt_unknown` per
    `unknown_report.rs:612-617`, plus template centralisation) and stays flat for
    5/6/8 (prelude → `RuntimePrelude`). All three predictions are unmeasured.
    Every step that regenerates the corpus must run
    `smelt smelt-unknown-report … --baseline blocker-logs/smelt-unknown-baseline-es-toolkit.json`
    and re-snapshot in the same commit on a decrease.
11. **Family 3's circular-reference cases (9 tests): do they fail on `getTag`,
    on `Object.is`, or on `stack` Map snapshot semantics?** I could not
    distinguish these by reading — `SmeltJsMap` has shared storage and
    `js_strict_eq` compares arrays/objects by `id`, so identity lookup *should*
    work. Re-measure after step 8 rather than planning against a guess.
