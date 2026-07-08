# es-toolkit generated-Rust codegen tail (non-coercion structural/trait/name/borrow)

Scope: the non-E0308 structural/trait/name/borrow tail of the whole-crate
es-toolkit build (`<es-toolkit>/dist-smelt`, 745 files). The dominant E0308
mismatched-types / coercion / callback / mutable-ref paths are explicitly out of
scope (owned by a sibling agent for a remeda regression). All measurements use
`smelt rust-diagnostics` on the release binary; dist-smelt regenerated before
each measurement.

Totals: **296 -> 292 errors** (warnings unchanged at ~440). No new error classes
introduced; no non-target class regressed.

## Fixed root causes

### 1. E0425 nested-closure double-capture of a shared cell (7 -> 5)

Sites: `src/debounce_1.rs:117,129` (`cannot find value smelt_capture_`).

Root cause (`emitter/list_query.rs`, closure emission): when a closure nested
inside an escaping shared-capture closure re-captures the same binding, the
source binding's rendered name is already `(*smelt_capture_x.borrow_mut())`. Two
spots mishandled that already-rendered form:

* The nested closure's target-local name was wrapped a second time
  (`format!("(*smelt_capture_{alias}.borrow_mut())")`), producing the invalid
  `(*smelt_capture_(*smelt_capture_timeout_id.borrow_mut()).borrow_mut())` which
  fails to resolve (E0425).
* The capture-clone prelude bailed out entirely (`return None`) for an
  already-rendered source name, so the nested closure never cloned the enclosing
  `Rc` cell — which, once the double-wrap was removed, would have surfaced as a
  moved-cell borrow error.

Fix: added `shared_capture_cell_name()` to recover the bare `smelt_capture_x`
cell from an already-rendered access. The target-name path now reuses the
already-rendered form verbatim; the prelude path clones the recovered `Rc` cell
(`let smelt_capture_x = smelt_capture_x.clone();`). General across any depth of
nested shared-capture closures.

Regression test: `nested_closure_reuses_enclosing_shared_capture_cell`
(part_1_tests.rs) — asserts the double-wrap substring never appears, the nested
`Rc` clone is emitted, and the assignment targets the single-wrapped cell.

### 2. E0631 optional-union `.length` mapper mismatch (5 -> 3)

Sites: `src/trimEnd_1.rs:33`, `src/trimStart_1.rs:31`
(`type mismatch in function arguments`).

Root cause (`emitter/numeric.rs`, length emission): for `.length` on an optional
whose inner type is a *concrete* union / type-parameter / erased class (e.g.
`string | string[]` -> `Option<SmeltUnion1093>`), the emitter used
`map_or(0, SmeltUnknown::len)`. `SmeltUnknown::len` has a `&SmeltUnknown`
receiver, but `.as_ref()` yields `&SmeltUnionN`, so the mapper signature
mismatches (E0631).

Fix: keep `SmeltUnknown::len` only when the inner type is literally
`Type::Unknown`. For a concrete union / type-param / erased-class inner, emit a
mapper that erases the borrowed value (`value.clone().into_smelt_unknown()`) and
inspects it (string char count / array len / length-bearing object), mirroring
the existing non-optional dynamic-length case. JS `.length` on such a value is a
genuinely dynamic property whose meaning depends on the runtime variant, so this
is a legitimate dynamic boundary (documented inline). The two sites did not
compile before, so this adds `into_smelt_unknown` erasure only at real
`.length`-on-dynamic-union boundaries, not to ordinary data flow.

Regression test: `optional_union_length_inspects_dynamically` (part_1_tests.rs).

## Deferred (root cause identified; fix is architectural or out-of-lane)

### E0425 async-`this` capture `smelt_capture_self` (4, `src/main.rs`, class `Semaphore::acquire`)

`emit_method` (core.rs) emits the method body via `emit_block` directly, skipping
`emit_shared_parameter_preludes`, so `smelt_capture_self` is never bound. But
merely emitting the prelude does not fix it: the method receiver is `&mut self`,
and the body's `new Promise(resolve => this.deferredTasks.push(resolve))` pushes
an **escaping** closure that captures `this` into a field. `&mut self` cannot be
moved into an `Rc<RefCell<>>` and shared with a closure that outlives the method
frame — emitting the prelude would only trade E0425 for borrow/lifetime errors.
The honest fix is interior-mutability class modeling (methods over
`Rc<RefCell<Self>>` or per-field shared cells so escaping closures can share
`this` by reference like JS). This is the same architectural root as the
sibling-tracked E0596 `self.semaphore` / `self.__data__` self-borrow cluster, so
it wants a coordinated class-model change, not a local patch.

### E0277 JS Set / dedup requires `Eq + Hash` (7: uniq_1, pullAt, pullAt_1)

`new Set(arr)` / `uniq` lowers via `list.iter().cloned().collect::<HashSet<_>>()`
(`emitter/list.rs::list_to_set_text`), which needs `T: Eq + Hash`. That fails for
`f64` (not `Eq`/`Hash`), for concrete unions (`SmeltUnion359`), and for unbounded
generic `T`. Adding `Eq + Hash` bounds is not a general fix: `f64`/float-bearing
instantiations can never satisfy them, and JS `Set` uses SameValueZero (NaN,
+0/-0, object identity) semantics that a Rust `HashSet` does not model. The
honest fix is a runtime JS-Set container that dedups by a hashable projection of
the erased JS value (`into_smelt_unknown` key) while storing the original `T`,
rewiring `Type::Set` codegen away from `HashSet` crate-wide. That is a
cross-cutting runtime + emitter change with broad regression surface across all
Set usage, out of proportion to a scoped tail fix; deferred with this design.

### E0609 / other E0277 / E0282 / E0631 / E0382 (heterogeneous semantic-modeling gaps or sibling-owned)

* E0609 `DebouncedFunction.apply` (5) — modeling `Function.prototype.apply` as a
  field access on a generated debounced-function struct; needs callable-object
  modeling.
* E0609 `SmeltMatch.result` (2, truncate) and `SmeltList.length` (2, unzipWith,
  inside a mistyped callback) — regex-match modeling and a callback-return
  type-inference tangle, respectively.
* E0609 `HttpError.name` (1) — missing generated error field.
* E0631 `matchesProperty` (3) — closure emitted with param `SmeltUnknown` where
  the target expects `Option<SmeltUnknown>`; this is closure/callback-argument
  coercion, in the sibling's lane.
* E0382 filter/partition/uniqBy/unionBy (4) — a value moved into `Some(x)` twice
  because a sub-expression is emitted once as a dead temp and once inline;
  copy-propagation / move-on-last-use, in the sibling's lane.
* E0277 escape/unescape `AsRef<str>` (2) — compound bug: the module-level
  `htmlEscapes` const is lowered to `SmeltUnknown::Null` inside the regex
  replacer, and the replacer closure returns `SmeltUnknown` instead of a
  `String`; needs module-const capture + replacer-return work.
* E0282 omit/omitBy/pickBy (7) — `Default::default().into_iter()` emitted with no
  inferable element type from an unmodeled key-enumeration builtin.

## Validation

* `cargo check --workspace`: clean.
* `cargo clippy -p smelt-codegen-rust --lib`: no new warnings from this change
  (2 remaining are pre-existing in call.rs / list_mutation.rs).
* `cargo test --workspace --exclude smelt-gui`: all green (includes the two new
  regression tests).
* Diagnostics: 296 -> 292; E0425 7 -> 5, E0631 5 -> 3; no class regressed.

Baseline: `blocker-logs/estk-baseline.md`; after: `blocker-logs/estk-after2.md`.
