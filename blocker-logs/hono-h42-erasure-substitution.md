# H42 — one render position, one erasure answer

Round 29, agent B. Campaign plan §D2 / H42, **option 1** (thread a substitution
through the value-rendering recursion). Two commits, as briefed: a mechanical
signature threading with zero behaviour change, then the rule change.

## The defect, restated from the code

Two rules governed the two sides of a map-and-collect and contradicted each
other (`crates/smelt-codegen-rust/src/emitter/coercion.rs`):

- `value_at_type_text`'s erasure arm erased a bare `Type::TypeParam` target
  **unconditionally, at any depth**. Taken when the entry's source is a typed
  value.
- `extract_value_text`'s `TypeParam` arm **respected scope**: where
  `current_function_has_type_param(name)` held it recovered the value as
  `<T as SmeltFromUnknown>::smelt_from_unknown(..)`. Taken when the entry's
  source is already `SmeltUnknown`.

A third rendering, `collected_container_type_text`, wrote the
`collect::<..>()` turbofish. H41a had made it erase unconditionally so it would
agree with the first rule; where an entry took the SECOND path the two
disagreed in the opposite direction.

Neither side could be fixed alone. The campaign plan recorded the measurement:
making the annotation scope-respecting *by itself* took the router slice from 15
errors to 27.

## What landed

### Commit 1 — `RenderScope`, threaded (zero behaviour change)

`crates/smelt-codegen-rust/src/emitter/render_scope.rs` (new, documented) defines
`RenderScope`, the **owned** companion of `TypeSubstitution`. It owns its
`HashSet<Symbol>` rather than borrowing one because the ambient scope
(`current_function_type_params`) is *computed* per position — it reads the
hoisted-item flag and the generic-suppression cell, both of which change while a
function is emitted — so it cannot be cached and lent by reference. Owning it
lets a call site write `&self.render_scope()` in argument position.
`RenderScope::substitution()` lends the borrowed `TypeSubstitution` whenever a
type has to be lowered, preserving `ScopeOrigin`.

Threaded through: `value_at_type_text`, `extract_value_text`, `extract`,
`callable_object_call_slot_text`, `inject_union_value_text`, and the four
record/function adapter helpers in `core.rs`. `value_at_type` binds one at the
top of its body. 238 ordinary emit sites pass `&self.render_scope()`, which is
exactly the ambient set those functions already consulted.
`project_union_value_text` was threaded by the mechanical pass and then
un-threaded: it never renders a value at a type-parameter position, so the
parameter would have been dead.

Proof of zero behaviour change: the Hono router slice regenerated
**byte-identical** across all 18 generated files (`diff -rq`), and every suite
stayed green.

### Commit 2 — the rule

Three edits, all of them the same query:

1. `value_at_type_text` gained an arm **above** the erasure arm: a
   `Type::TypeParam` target that `scope.spells(name)` goes to
   `extract_value_text` (recovering a real `T`) instead of erasing. This mirrors
   the arm `value_at_type` already had for the operand-shaped entry point, so
   the two spellings of the coercion no longer disagree either.
2. `collected_container_type_text` renders under the threaded `scope` instead of
   `TypeSubstitution::erased()`. H41a's blanket erasure is gone and the helper's
   docstring — which explicitly told the reader not to tighten it without
   reading H42 — was rewritten.
3. `list_items_render_as_unknown` asks the threaded scope rather than the
   ambient one, so the `extract_value_text` arm that uses it agrees with the
   position too.

### The fourth side: a callee's parameter is not the caller's scope

Making the three sides agree at the *caller's lexical* scope fixed the slice's 6
errors and immediately produced **8 new ones** — the "flat 8" family the campaign
plan predicted:

```
find_middleware(middleware: SmeltRecord<String, SmeltList<SmeltUnknown>>, ..)
                                        ^ the callee's own T erased
  called with  SmeltRecord<String, SmeltList<T>>
                                             ^ the CALLER's T, because
                                               RegExpRouter<T>::add declares one
```

`smelt_hir::Symbol` is name-interned, so a caller's `T` and a callee's `T` are
the same key: rendering a callee-owned target under the caller's scope silently
captures the caller's unrelated parameter. This is the hazard
`crate::type_substitution`'s module docstring already names for *type* rendering;
value rendering had no equivalent because it had no threaded scope to narrow.

`RenderScope::narrowed_to` takes the intersection, and
`FunctionEmitter::callee_parameter_render_scope` builds it from the callee's own
emission scope (`callee_class_type_params` ∪ `callee_free_function_type_params`,
each of which already answers the empty set for a callee the crate-wide gate
demoted). Rung 7b of the static-call argument ladder
(`StaticArgumentKind::Coerced`, the only rung that coerces to a callee's
*declared* parameter type) renders under it.

A type parameter is spellable at such a position only where **both** sides spell
it: the caller's item must declare the name (or the body cannot write it) and the
callee must have emitted it as a real Rust generic (or its slot is
`SmeltUnknown`). Taking the callee's scope alone would spell a `T` the caller
never declared; taking the caller's alone is the 8 errors above.

## Measurements

Every number below was taken with a `smelt` freshly built from the tree it is
attributed to, on a clean generated crate.

### Hono router slice (`third_party/hono/dist-routers`)

| stage | errors |
| --- | ---: |
| before (integration head, reproduced here) | 6 |
| after commit 1 (byte-identical output) | 6 |
| after the three-sided rule, before the callee-seam narrowing | 8 |
| **after commit 2** | **0** |

The 6 before: 2 × `E0277` (a record built from an iterator whose items still
carry `T`), 2 × `E0308` `expected type parameter T, found SmeltUnknown`,
2 × `E0308` `SmeltList<(T, …)>` vs `SmeltList<(SmeltUnknown, …)>`.

### Hono whole crate (`third_party/hono/dist-smelt`, 33 files)

Clone at `eebdf7be39abf0a872671835ccce0c4f03ea497a` + `.github/compat/hono/.`,
`smelt build`, `cargo check`. Both columns measured in this worktree, the
"before" with the commit-1 binary.

| code | before | after | delta |
| --- | ---: | ---: | ---: |
| E0107 (generic argument count) | 136 | 136 | 0 |
| E0308 (mismatched types) | 138 | 134 | **−4** |
| E0609 (no field) | 17 | 17 | 0 |
| E0121 (type placeholder) | 10 | 10 | 0 |
| E0382 (use of moved value) | 8 | 8 | 0 |
| E0277 (trait bound) | 7 | 5 | **−2** |
| E0063 (missing field) | 5 | 5 | 0 |
| E0425 (unresolved name) | 2 | 2 | 0 |
| E0599 (no method) | 1 | 1 | 0 |
| E0271 (associated type mismatch) | 1 | 1 | 0 |
| **total** | **325** | **319** | **−6** |

The committed round-28 table (`hono-current.md`) reads 326; the extra one is an
error carrying no rustc code, which this tally skips. Against that published
baseline the crate is **326 → 320** on the same accounting, or 325 → 319 on
codes alone.

The two remaining `E0277`s and the residual `E0308`s are not H42: they are
`SmeltUnion*` / `SmeltHeaders` / `(SmeltUnknown, RouterRoute)` families, and the
136 `E0107`s are the type-parameter-defaults family the round-29 brief defers.

### Gates

| gate | result |
| --- | --- |
| examples invariant (`--fail-on-regression`) | avoidable **0**, delta +0 in all three categories |
| es-toolkit ratchet (`--fail-on-regression`) | avoidable **31701** vs baseline 31701, delta +0 — equal, so no re-snapshot |
| remeda generated `cargo test` | **1789 passed / 0 failed** |
| radash generated `cargo test` | **84 passed / 0 failed** |
| `cargo test -p smelt-codegen-rust` | 1054 passed / 0 failed |
| `cargo test -p smelt-transpiler` | 117 passed / 0 failed (incl. the 86-fixture end-to-end suite) |
| `cargo test -p smelt-frontend-ts --no-default-features` | 1085 passed / 0 failed |
| `cargo clippy --all-targets` | no new findings |

## The regression test, and why it is a runtime tier

`crates/smelt-codegen-rust/tests/type_param_render_scope_runtime.rs` (registered
in the `values` shard of `.github/workflows/runtime-tiers.yml`) runs one generic
class that stores the same `T` three ways — the flat map-and-collect, the nested
record-of-record, and the tuple-list rebuild — plus a generic free function
`pick<T>(table: Record<string, T[]>, ..)` for the callee seam. Verified against
Node 22: the tier's expectations are that program's output byte for byte.

Against the commit-1 emitter the tier fails with **exactly the three shapes**:

```
E0277  SmeltRecord<String, SmeltList<SmeltUnknown>> from (String, SmeltList<T>)
E0277  SmeltRecord<.., SmeltList<(SmeltUnknown, ..)>> from (.., SmeltList<(T, ..)>)
E0308  expected type parameter `T`, found `SmeltUnknown`
E0308  SmeltList<(T, ..)> vs SmeltList<(SmeltUnknown, ..)>
```

**Deviation from the brief, stated plainly.** The brief asked for end-to-end
fixture 89. It was written, generated and verified against Node — and then moved,
because every shape it covers needs a value that has ALREADY crossed into
`unknown` (that is what makes an entry take the recovery path while the container
around it takes the coercion path, which is the pairing the two rules disagreed
about). Source `unknown` is a genuine dynamic boundary under CLAUDE.md, but
`unknown_report`'s line-based classifier counts a local declared
`SmeltRecord<String, SmeltUnknown>` as avoidable erasure, and the examples corpus
is a hard `avoidable == 0` invariant. Measured: **12** such lines, every one of
them the fixture's own seeds (`flat_seed`, `nested_seed`, `row_seed` and the
temporaries feeding them); a first draft that used `Object.create(null)` and
`unknown`-returning helpers measured 15. Reshaping the seeds to erase through
`into_smelt_unknown` (a boundary marker) moved 3 of them and no more.

That left three options: break the invariant, reclassify `unknown`-typed locals
as a legitimate boundary in `classify_line` (correct by CLAUDE.md's own wording,
but a metric change that would reclassify thousands of es-toolkit lines and has
no business inside an H42 commit), or follow the round-28 precedent
(`hono-round28-examples-invariant.md`, fixtures 80 and 85): the erased subject
moves to an executing tier, where the assertion is the VALUE rather than the
spelling. I took the third. No number was accepted that a fixture placement could
have hidden — the corpus fixture directory and its `END_TO_END_EXAMPLES` entry
were removed, so the invariant is 0 by construction and not by re-snapshot.

## SmeltUnknown delta

No new `SmeltUnknown` conversion was introduced; the change **removes** erasure.
Where a render position can spell a type parameter, the value is now carried as
that real Rust generic instead of being boxed into the runtime carrier — the
"concrete, then scoped generics, then erasure" order CLAUDE.md asks for. The
three baselines are flat (examples +0, es-toolkit +0, and remeda's generated
crate still passes 1789/0); none of them contains the shape this fixes, which is
why the movement shows up in the Hono table and not in the erasure metric.

## What this leaves for the next round

- `RenderScope::erased()` exists and is exercised by unit tests, but no
  production value render reaches it yet: a hoisted module item and a demoted
  callee both arrive as an empty *ambient* scope instead. Collapsing those two
  onto the explicit constructor would make the provenance readable in a `Debug`
  dump; it changes no bytes today.
- `callee_parameter_render_scope` is wired into rung 7b of the static-call
  argument ladder only. The other callee-parameter coercion sites
  (`call.rs:1064/1182/1482/2056/3213`, the rest-parameter pre-pass and the
  indirect-call paths) still render at the caller's ambient scope. Nothing in the
  three corpora or in Hono reaches a disagreement there today — that is why they
  were left alone rather than changed blind — but they are the same seam and the
  same narrowing applies if one ever does.
- Found and NOT fixed, unrelated to H42: writing through a local alias of a
  nested record loses the write. In the first draft of the fixture,
  `const table = this.nested[group]; table[path] = list;` (with no write-back)
  produced an empty table in generated Rust where Node produced two entries. The
  tier's fixture assigns `this.nested[group] = table` explicitly to avoid it.
  This is the projected-receiver-writeback family (H31/H19); it deserves its own
  reproduction.
