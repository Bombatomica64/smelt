# TS Frontend Lowering — Architecture Refactor Plan

**Status:** FOCUSED PASS COMPLETE. After host-runtime specialization stabilized,
the first low-risk module splits landed: pure support helpers now live behind a
real `support` module seam, and constructor-function lowering is a real focused
module rather than a textual include. The larger semantic re-division below
remains an incremental backlog and is intentionally not bundled with the
specialization feature.

**Scope:** `crates/smelt-frontend-ts/src/lowering*` re-division by semantic concern. The codegen emitter (`crates/smelt-codegen-rust/src/emitter/`) is already cleanly modularized and is covered only as a contrast / secondary backlog at the end. MIR closures are noted for completeness.

---

## 1. The central structural fact

`crates/smelt-frontend-ts/src/lowering.rs` is the parent module. It declares **three real submodules**:

```rust
mod ambient_globals;
mod stdlib;
mod stdlib_dispatch;
```

…and then **textually `include!`s 22 files** into the bottom of the same file (lines ~431-455):

```rust
include!("lowering/builder_part01.rs");
... builder_part02..08 ...
include!("lowering/call.rs");
include!("lowering/collections.rs");
... builder_part09..15 ...
include!("lowering/types.rs");
... builder_part16..18 ...
include!("lowering/constructor_function.rs");
include!("lowering/helpers_part01.rs");
```

Consequences that drive the whole plan:

1. **Every `builder_partNN.rs` / `call.rs` / `collections.rs` / `types.rs` / `constructor_function.rs` file is just a continuation of one giant `impl<'ctx> ModuleBuilder<'ctx>` block.** They are NOT modules. They share one flat method namespace and one ~60-field `ModuleBuilder` struct (lines 308-417 of `lowering.rs`). The `partNN` numbering encodes *history of growth*, not *responsibility*.
2. **The `mod`-declared files (`stdlib.rs`, `stdlib_dispatch.rs`, `ambient_globals.rs`) are the proven target shape.** `stdlib.rs` opens with `impl ModuleBuilder<'_> { ... }` and `use super::{ModuleBuilder, SmeltError, stdlib_dispatch};`, reaching the *private* `ModuleBuilder` fields directly. This works because in Rust a **child module can read private items of its ancestor module**. This is the migration lever: any `impl ModuleBuilder` method can move from an `include!`d file into a real `mod` with no field-visibility change to the struct.
3. **`helpers_part01.rs` holds free `fn`s (no `self`)** — the cheapest possible extraction, because free functions don't even need the `impl`/`use super::ModuleBuilder` wrapper.

So the refactor is fundamentally: **turn `include!`d impl-fragments into `mod`-declared `impl ModuleBuilder` submodules grouped by semantic concern**, using the exact pattern `stdlib.rs` already proves.

---

## 2. Inventory table (current files → LOC → concerns held today)

LOC and themes cross-verified by reading file headers + `codedb_outline` of every file.

| Current file | LOC | Wiring | Semantic concern(s) held today | Coherence |
|---|---|---|---|---|
| `lowering.rs` | ~456 | parent | Entry (`to_hir*`), `ModuleBuilder` struct + ~60 fields, shared support types (`TestMatcher`, `ConstLiteral`, `ConstCollection`, `LocalCallback`, `RestParam`, `InterfaceHeritageRef`, …), `include!`/`mod` wiring | Coherent (it is the spine) |
| `builder_part01.rs` | 2734 | include | Module init (`new`, `visible_items`, `visible_const_literals`), program entry (`program`), top-level statement routing, module-global scan, const-collection prep for nested bodies | Coherent (init + orchestration) |
| `builder_part02.rs` | 1162 | include | Arrow-function *const* declaration lowering (`arrow_function_const_declaration*`), async Promise-return enforcement, param destructuring | Coherent |
| `builder_part03.rs` | 1754 | include | Named **function declaration** lowering, assertion/predicate guard returns, overload dispatch, body lowering | Coherent |
| `builder_part04.rs` | 1587 | include | **Type alias + interface** declaration lowering, heritage resolution, callable-object recognition | Coherent |
| `builder_part05.rs` | 1155 | include | **Test framework**: `describe`/suite lowering, hook inheritance, nested-suite flattening | Coherent |
| `builder_part06.rs` | 2894 | include | **Test assertions**: Node `assert.*`, Vitest `expect().matcher()`, SameValue/NaN/identity rules | Mixed (matchers + JS-semantics rules) |
| `builder_part07.rs` | 297 | include | **Control flow**: C-style `for`, for-of/for-in pattern extraction | Coherent |
| `builder_part08.rs` | 1871 | include | **`new` expressions / constructors**: Set/Map/Promise/Date/Intl/class dispatch | Coherent |
| `call.rs` | 3603 | include | **Call-expression dispatch hub**: builtin/stdlib routing, member calls, optional chaining, error fns, namespace-alias calls, nested/curried call patterns | Mixed (large hub) |
| `collections.rs` | 590 | include | **Typed Map/Set method** lowering via stdlib metadata dispatch | Coherent |
| `builder_part09.rs` | 2133 | include | **`instanceof` + `typeof` narrowing**, ambient-global probes, nullish/undefined guards, type assertions; **plus** Promise/timer combinators and **Date** constructor/member lowering | Mixed (guards + Promise/Date tangled in) |
| `builder_part10.rs` | 1194 | include | **Stdlib statics**: Math, Number (parse/predicate/toString), URL, Intl, crypto, Node process | Coherent (per-namespace) |
| `builder_part11.rs` | 1740 | include | **Object/Dict statics** (`Object.is/keys/create/entries/fromEntries/hasOwn`), Buffer, Node path, lodash; **plus** several String methods (case/normalize/trim/affix/search/replace/repeat) | Mixed (Object + stray String methods) |
| `builder_part12.rs` | 756 | include | **Type assignability/variance** (`type_assignable_to*`, numeric/map-key compatibility); **plus** String/Array methods (pad/charAt/join/contains) | Tangled (two unrelated halves) |
| `builder_part13.rs` | 7409 | include | **List/Array callback methods** (concat/map/reduce/filter/search/at/entries) **+ the entire callback→closure subsystem** (~3500 LOC: arrow/function callback lowering, capture collection, callback body synthesis, guard narrowing, throw analysis) | Tangled/Large (~40% list methods / ~60% callbacks) |
| `builder_part14.rs` | 3558 | include | **Collection construction** (Array/Set/Map/Object ctors, spreads) **+ binary/logical/nullish operators** with fallback narrowing **+ object-literal lowering** (spread, contextual record typing) | Mixed (3-way) |
| `builder_part15.rs` | 1654 | include | **Member access** (static/dynamic/computed/chain, namespace member, Math/Number/process members) **+ assignment/update/destructuring** statements | Mixed (50/50 member-read vs lvalue-write) |
| `builder_part16.rs` | 1215 | include | **Identifier / reference value lowering**: name interning, identifier exprs, builtin/global/module-global values, builtin **closure values** (cast/predicate/unary), const-item/const-collection materialization, `span` helpers | Coherent |
| `builder_part17.rs` | 318 | include | **Interface/symbol + property-key metadata**: property-key→symbol, implements validation, interface/type-alias lookup, `expr_ty`/`local_ty`/`item_ref` accessors | Coherent |
| `builder_part18.rs` | 218 | include | **Generic type-parameter scoping** + substitution (`push/pop_type_parameter_scope`, `substitute_type_params`, substituted fields/methods) | Coherent |
| `types.rs` | 2655 | include | **TypeScript type→HIR lowering** (`ts_type_to_hir` and the full annotation/reference/field-inspection surface) | Coherent |
| `constructor_function.rs` | 828 | include | **Constructor-function idiom → synthesized class** (`function Foo(){this.x=…}` + `Foo.prototype.m=…`) | Coherent |
| `stdlib.rs` | 2591 | **mod** | Focused stdlib method lowering (`Object.assign`, Array methods, Vitest mocking, collection slice). **Reference pattern** for the migration. | Coherent |
| `stdlib_dispatch.rs` | 203 | **mod** | Shared `RuleId` dispatch + pure-Math const folding (`call_rule`, `pure_math_call`, `static_member_rule`) | Coherent |
| `ambient_globals.rs` | 144 | **mod** | Ambient-global alias recognition + `typeof globalThis` probe erasure | Coherent |
| `helpers_part01.rs` | 205 | include | **Free `fn` helpers** (no `self`): `item_name`, `unknown_kind_from_typeof`, `sanitize_test_name`, `statement_terminates`, … | Coherent (free fns) |

**Where concerns are tangled purely by accretion (the refactor targets):**

- **`builder_part13`** — list-method lowering and the callback/closure subsystem are two distinct concerns fused into one 7.4k-line file.
- **`builder_part12`** — type-assignability helpers vs string/array method lowering: two halves with nothing in common.
- **`builder_part09`** — type guards (`instanceof`/`typeof`/nullish) vs Promise/timer combinators vs Date construction/methods.
- **`builder_part14`** — collection construction vs binary/logical operators vs object-literal spread.
- **`builder_part11`** — Object statics vs stray String methods (also scattered into 10 and 12).
- **`builder_part15`** — member *reads* vs assignment/destructuring *writes*.
- **`call.rs`** + **`builder_part06`** — large dispatch hubs (call routing; test-assertion dispatch) that mix many sub-families.

Note the **cross-file scatter**: String-method lowering lives in part10, part11, part12, and stdlib.rs; Date lowering in part08 and part09; Object statics in part11 and stdlib.rs; member access in part15 and part16. These are the clearest signals that the split is historical, not semantic.

---

## 3. Proposed target module layout (grouped by semantic concern)

Target: replace the `partNN` numbering with a `mod`-declared tree under `lowering/`, each a real submodule containing `impl ModuleBuilder<'_>` (or free fns) and `use super::*` for shared types. `ModuleBuilder` and its fields stay in `lowering.rs` unchanged (private fields remain reachable by child modules).

```
lowering.rs                      # spine: to_hir*, ModuleBuilder struct + fields,
                                 #   shared support types, `mod` wiring (no include!)
lowering/
  support.rs                     # FREE FN helpers (from helpers_part01) + shared
                                 #   support types if peeled off the spine
  module_init.rs                 # ModuleBuilder::new, visible_items/const_literals,
                                 #   program(), top-level statement routing,
                                 #   module-global scan/collection prep        (part01)
  decls/
    functions.rs                 # named fn decls + assertion/predicate returns (part03)
    arrows.rs                    # arrow-const declarations                    (part02)
    types_iface.rs               # type alias + interface declarations         (part04)
    constructor_function.rs      # constructor-fn idiom -> class  (already coherent file)
  stmt/
    control_flow.rs              # for / for-of / for-in                       (part07)
    assignment.rs                # assignment/update/destructuring stmts   (part15 lower half)
  expr/
    literals.rs                  # array/object/set/map *construction*, spreads,
                                 #   contextual record typing            (part14 construction)
    operators.rs                 # binary / logical / nullish / unary / in / delete,
                                 #   fallback narrowing                  (part14 operators)
    member_access.rs             # static/dynamic/computed/chain member reads,
                                 #   namespace/Math/process members      (part15 upper half)
    references.rs                # identifier/global/builtin/module-global values,
                                 #   builtin closure values, const materialization (part16)
  callbacks/                     # the extracted closure subsystem        (part13 ~60%)
    mod.rs                       # callback_expression dispatch + arrow/function entry
    capture.rs                   # capture-name collection, remap, ClosureCapture build
    body.rs                      # callback_*_to_body_expr family, block/stmt synthesis
    narrowing.rs                 # guard narrowing, typeof/nullish/truthy in callbacks
    throw.rs                     # throw detection + uncaught-throw analysis
  stdlib/                        # value-call lowering, grouped by API family
    mod.rs                       # shared dispatch entry; re-exports
    call_dispatch.rs             # call_expression hub + builtin routing      (call.rs)
    collections.rs               # typed Map/Set methods           (collections.rs as-is)
    list_methods.rs              # Array.prototype methods         (part13 ~40% + list bits)
    strings.rs                   # ALL String methods consolidated (from 10/11/12 + stdlib)
    numbers_math.rs              # Math/Number/parse/predicate     (part10 math/number)
    objects.rs                   # Object.* statics, fromEntries, hasOwn        (part11)
    date.rs                      # Date ctor + members             (from part08/part09)
    promise.rs                   # Promise combinators/ctor + timers (from part09)
    intl_url_node.rs             # Intl, URL, crypto, Buffer, Node path/process (part10/11)
    interop.rs                   # lodash/strapi/Vitest-mock helpers (from 11/13/stdlib)
    dispatch_rules.rs            # stdlib_dispatch.rs (already a mod)
    object_assign.rs             # remaining focused stdlib.rs methods
  guards/
    instanceof.rs                # instanceof + Date-value provenance          (part09)
    typeof_narrow.rs             # typeof/nullish/undefined narrowing, ambient probes (part09)
    assertions.rs                # type assertions / casts                     (part09)
  testing/
    suites.rs                    # describe/test suite lowering                (part05)
    matchers.rs                  # expect()/assert.* matchers + SameValue rules (part06)
  ty/
    annotations.rs               # ts_type_to_hir + annotation/reference lowering (types.rs)
    inspection.rs                # field/method/type-predicate inspection      (types.rs)
    assignability.rs             # type_assignable_to* + compatibility    (part12 upper half)
    generics.rs                  # type-parameter scoping + substitution        (part18)
    metadata.rs                  # property-key/symbol + interface lookup + accessors (part17)
  ambient_globals.rs             # already a mod
```

Notes:
- The grouping deliberately **consolidates the scattered families** (String, Date, Object, member access) into one home each. That is the highest-value outcome for "safer LLM-generated changes," because an LLM editing string lowering then sees all of it in `stdlib/strings.rs` instead of four `partNN` files.
- Depth is kept shallow (one or two levels). The directories (`decls/`, `expr/`, `callbacks/`, `stdlib/`, `guards/`, `testing/`, `ty/`) map 1:1 to the lowering phase an editor is reasoning about.
- File sizes target roughly 300-1500 LOC; `callbacks/` and `stdlib/list_methods.rs` will be the largest survivors but each is single-concern.

---

## 4. Safe, incremental migration sequence (one PR per step, post-feature-phase)

Each step is **pure code-motion, no behavior change**, and must leave `cargo check`, `cargo clippy`, and `cargo test` green (per `CLAUDE.md` "always run"). The enabling mechanic for every step: convert an `include!("lowering/X.rs")` line into a `mod x;` line, and wrap the moved `impl` fragment in `impl ModuleBuilder<'_> { … }` + `use super::*;` — exactly as `stdlib.rs` already does. Move methods in named clusters so each diff is reviewable.

Order is chosen so the **cheapest, lowest-risk, most-mechanical moves come first**, and the **shared-state-entangled hubs (`call.rs`, `builder_part13` callbacks) come last** after the module skeleton exists.

**Step 1 — Free-fn helpers → real `mod support`.** Move `helpers_part01.rs` from `include!` to a declared `mod support;` (rename file `support.rs`). These are free `fn`s with no `self`, so the only change is adding `use super::*;` for the HIR types they reference and adjusting visibility to `pub(super)`. Zero method-namespace impact. *This proves the include!→mod conversion end-to-end on the safest possible file.*

**Step 2 — Already-coherent leaf files → `mod` (no content change).** Convert these `include!`s to `mod` declarations one PR each, wrapping the existing fragment in `impl ModuleBuilder<'_> { … } use super::*;`. They are already single-concern, so this is rename-only:
- `builder_part18.rs` → `ty/generics.rs`
- `builder_part17.rs` → `ty/metadata.rs`
- `builder_part07.rs` → `stmt/control_flow.rs`
- `constructor_function.rs` → `decls/constructor_function.rs`
- `builder_part16.rs` → `expr/references.rs`
- `types.rs` → `ty/annotations.rs` (+ split inspection later; first pass: move whole file)

Each PR touches one `include!` line and one file path. Risk: only that some `fn` was relied on by name from another `include!` fragment — but because all fragments compile into the same `impl`, moving one into a child `mod` keeps the methods callable via `self.` unchanged. Verify with `cargo check` after each.

**Step 3 — Declaration cluster → `decls/`.** Move `builder_part02/03/04` into `decls/arrows.rs`, `decls/functions.rs`, `decls/types_iface.rs`. Still single-concern each; mechanical. Do `module_init.rs` (part01) last in this group because `program()` references the most other methods (all reachable via `self.`, so still mechanical, just the largest review surface).

These first three steps establish the `mod` skeleton and convert ~10 already-coherent files with near-zero risk, before any *content* re-division.

**Step 4 — Split the tangled-but-separable files along their existing seam.** Each of these has two/three internally-coherent halves with a clean boundary; split by moving whole method clusters:
- `builder_part12` → `ty/assignability.rs` (type_assignable_to*) + fold String/Array methods into `stdlib/strings.rs` / `stdlib/list_methods.rs`.
- `builder_part15` → `expr/member_access.rs` (reads) + `stmt/assignment.rs` (writes).
- `builder_part14` → `expr/literals.rs` (construction) + `expr/operators.rs` (binary/logical/nullish/unary).
- `builder_part09` → `guards/instanceof.rs` + `guards/typeof_narrow.rs` + `guards/assertions.rs`, moving Promise/timer to `stdlib/promise.rs` and Date to `stdlib/date.rs`.

**Step 5 — Consolidate the scattered stdlib families.** Now that homes exist, gather String methods (from old 10/11/12/stdlib) into `stdlib/strings.rs`; Object statics into `stdlib/objects.rs`; Math/Number into `stdlib/numbers_math.rs`; Date into `stdlib/date.rs`; Intl/URL/Node/Buffer into `stdlib/intl_url_node.rs`; lodash/strapi/Vitest into `stdlib/interop.rs`. `collections.rs` and `stdlib_dispatch.rs` are already coherent — just relocate under `stdlib/`.

**Step 6 — Testing cluster.** `builder_part05` → `testing/suites.rs`; `builder_part06` → `testing/matchers.rs`.

**Step 7 — Extract the callback subsystem from `builder_part13` (highest value, do with most care).** Split the ~3500-LOC callback machinery into `callbacks/{mod,capture,body,narrowing,throw}.rs`, leaving list-method lowering as `stdlib/list_methods.rs`. Do this last because it is the densest internal cross-referencing; move it as one cluster (it is internally coherent — see §2) into one `callbacks/mod.rs` first, then sub-split in a follow-up PR if desired.

**Step 8 — Split the `call.rs` dispatch hub.** Carve `call_expression` + builtin routing into `stdlib/call_dispatch.rs`, leaning on the now-populated `stdlib/*` family modules. Last because it is the routing nexus that calls into nearly every other family; safest once the families already live in their target homes.

**Step 9 — Remove the `include!` block entirely** from `lowering.rs`, leaving only `mod` declarations. Optionally peel shared support *types* out of `lowering.rs` into `support.rs` if the spine is still large.

After each step: `cargo check` → `cargo clippy` → (before the PR) `cargo test`. Because the repo uses bare `cargo` (default-members excludes `smelt-gui`; do **not** add `--workspace`), the loop stays fast.

---

## 5. Risk notes — mechanical relocation vs entangled shared state

**Pure mechanical relocation (low risk).** Moving an `impl ModuleBuilder` method (or free fn) from an `include!`d fragment into a child `mod` does **not** change how it is called: methods are still invoked as `self.foo(...)` and resolve against the single `impl`. Private `ModuleBuilder` fields stay reachable because child modules can read ancestor private items (proven today by `stdlib.rs`). So Steps 1-3 and the leaf moves in 4-6 are mechanical: the only failure modes are (a) a missing `use super::Foo;` import in the new module (compile error, caught immediately by `cargo check`), and (b) a visibility tweak — methods called from a *sibling* child module must be `pub(super)` (also a clean compile error, no behavior risk).

**Entangled shared state (higher review cost, still behavior-preserving).** The real entanglement is the **~60-field `ModuleBuilder` struct**, not the methods. Several method clusters mutate overlapping fields:
- **Callbacks (Step 7)** touch `local_callbacks`, `narrowed_locals`, `type_param_scopes`, `current_*` (async/return_ty/generator/arguments), and the capture machinery. Moving them to `callbacks/` is still pure motion (fields stay on the struct, accessed via `self.`), but the cluster is large and densely self-referential — move it as one unit, do not interleave with other steps.
- **`call.rs` (Step 8)** dispatches into every stdlib family and reads narrowing/local state. It must move *after* the family modules exist so it only ever forwards to already-relocated methods.
- **`narrowed_locals` / `type_param_scopes` / `current_*`** are read across guards, callbacks, operators, and member access. No step *changes* this sharing; the plan keeps all such state on `ModuleBuilder` in `lowering.rs`. A deeper future refactor (out of scope here) could group these into sub-structs (e.g. a `NarrowingState`, a `FunctionScopeState`) — but that is a *behavioral-surface* change and must NOT be bundled into the code-motion PRs.

**Explicitly out of scope for these PRs (do not bundle):** changing field grouping/ownership on `ModuleBuilder`; merging the four equality/identity helper families; any `SmeltUnknown`/`Type::Undefined` work; touching generated-file emission. Those are feature/semantics changes; the architecture pass is code-motion only.

**Codegen emitter (secondary, mostly already done).** `crates/smelt-codegen-rust/src/emitter/` is already a clean `mod` tree (`core.rs`, `call.rs`, `call_runtime.rs`, `coercion.rs`, `control_flow.rs`, `list_query.rs`, `map.rs`, `list*.rs`, `set.rs`, `strings*.rs`, `numeric.rs`, `types.rs`, `place.rs`, `literals.rs`, `tuple.rs`, `rendered_value.rs`). The only oversized survivors are `core.rs` (~4429 LOC: emitter engine + local naming + reachability + record adapters + method-signature lookup) and `call_runtime.rs` (~2831). `lib.rs` itself notes "large emitters will be split after behavior stabilizes." A future, separate pass could split `core.rs` into `core/{engine,naming,reachability,record_adapters,method_lookup}.rs` — but the emitter is **not** the priority; the `include!`-into-one-impl frontend is.

**MIR closures (no action needed).** `crates/smelt-mir/src/lower/closures.rs` (~770 LOC) is a single coherent subsystem with four documented invariants (escaping→ByValue capture; transitive escape; uniform throwing-ABI `Type::Function{may_throw}`; capture-metadata sync). It is well-sized and self-contained — leave as-is.

---

## 6. First three steps to do first (recommendation)

1. **`helpers_part01.rs` → `mod support;`** — converts the safest file (free fns) and validates the whole `include!`→`mod` mechanic.
2. **Coherent leaf files → `mod`** (`builder_part18`→`ty/generics.rs`, `builder_part17`→`ty/metadata.rs`, `builder_part07`→`stmt/control_flow.rs`, `constructor_function.rs`→`decls/constructor_function.rs`) — rename-only conversions, one `include!` line each.
3. **`builder_part16` → `expr/references.rs` and `types.rs` → `ty/annotations.rs`** — two coherent, larger files moved whole, establishing the `expr/` and `ty/` directories before any content splitting.

These three steps build the `mod` skeleton with zero content re-division and zero behavior risk, leaving the tangled hubs (`builder_part13` callbacks, `call.rs`) for last once the destination modules exist.
