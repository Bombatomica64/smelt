# Module-level mutable state (mutable globals)

Implements the last known gate before es-toolkit's first whole-crate transpile:
`smelt build` aborted at `src/compat/util/uniqueId.ts` on
`let idCounter = 0; ... ++idCounter` ("only local, field, and index expressions
can be assigned"). Module-level `let`/`var` bindings mutated from hoisted item
bodies now lift to first-class "mutable globals" backed by per-test-thread
`thread_local!` cells.

## Design

- **HIR**: `Item::MutableGlobal` (name, primitive type, literal initializer,
  visibility, span) plus two expression kinds:
  - `GlobalGet { item }` — read, typed as the binding's type.
  - `GlobalSet { item, value }` — store; evaluates to the stored value so
    `++`/`+=` compose as expressions.
- **Frontend classification** (`collect_mutable_globals`,
  `crates/smelt-frontend-ts/src/lowering/module_init.rs`): a module-level
  `let`/`var` binding lifts iff it is mutated (reassignment, `++`/`--`,
  compound assignment) inside a *hoisted item body*: a top-level function
  declaration, class declaration, or `const` arrow/function initializer —
  the positions that lower to items with no module-body local to assign
  through. Mutations written directly in module-body statements (including
  inline `forEach` callbacks and vitest `it()` callbacks) keep today's
  module-body-local path, byte-identical. `var` is treated like `let`.
- **Desugaring** (`stmt/assignments.rs`): `x = e` → `GlobalSet(x, e)`;
  `x op= e` → `GlobalSet(x, GlobalGet(x) op e)`; `++x` →
  `GlobalSet(x, GlobalGet(x) + 1)` (value = new); `x++` in value position
  captures `GlobalGet(x)` in a temp `let`, emits the store as a side
  statement, and evaluates to the temp (value = old). Statement-position
  updates skip the old-value temp. A same-named local (function body or
  replayed test setup declaring its own binding) always shadows the global.
- **Same-declaration recognition**: the lifted binding's *own* declaration —
  in the module body or replayed as top-level test setup into a test body —
  is recognized by binding span (`is_lifted_global_declarator`) and skipped,
  so reads/writes resolve to the global. A different same-named declaration
  (other span) still creates its ordinary shadowing local.
- **Cross-module**: the lifted item is a module item; exports/imports resolve
  through the existing item-visibility machinery, so importer reads/writes
  reference the same `ItemId` and the same cell.
- **MIR**: `Mir.globals: Vec<MirGlobal { name, ty, init }>` populated by
  `lower_globals`; `Rvalue::GlobalGet { global: u32 }` /
  `Rvalue::GlobalSet { global, value }` (Set's result is the stored value).
  No new `Place` variant — global writes never reach `lower_place`.
- **Codegen** (`emit_mutable_globals`, gated on `!mir.globals.is_empty()`):
  one `thread_local!` per global, named `SMELT_GLOBAL_<NAME>_<index>`
  (index-disambiguated across modules; program-specific, never in the fixed
  runtime-symbol registry). Copy primitives use
  `Cell<f64/i64/bool> = const { Cell::new(init) }`; strings use
  `RefCell<String> = RefCell::new("…".to_owned())` (owned init cannot be
  `const`). Reads: `.with(Cell::get)` / `.with(|v| v.borrow().clone())`.
  Writes hoist the operand and evaluate to it. **Per-test semantics**: every
  `#[test]` runs on its own thread, so each test observes fresh module state
  initialized from the source literal — deterministic, mirroring vitest's
  per-file module isolation.

## V1 constraints (named blockers)

- Non-literal initializer → "module-level mutable binding initializer must be
  a literal for now".
- Non-primitive type (not Float/Int/Bool/String) → "module-level mutable
  bindings support primitive types for now".

Both fire only for bindings that classify as mutable globals — shapes whose
function-body writes previously always aborted MIR with the generic
"only local, field, and index expressions can be assigned", so the named
blockers strictly improve the failure without regressing anything that
transpiled before. Two pre-existing tests were updated accordingly:
`lowers_module_mutable_default_options_accessors` (date-fns `defaultOptions`
object initializer: HIR-lowered before but could never pass MIR; now the named
initializer blocker) and
`vitest_describe_expression_setup_is_replayed_into_nested_tests` (now lifts
`clock`, 3 module items, and the replayed setup observes the real global).

### Scope refinement vs. the original design note

The design said "mutated anywhere in the crate". Implemented as "mutated in
any hoisted item body" because module-body-only mutations already lower
correctly as module-body locals; lifting them would have imposed the literal
initializer constraint on working code (4 pre-existing codegen/snapshot tests
went red under the broad rule; all green under the refined rule). For valid
ES modules the two rules agree on cross-module behavior: assigning to an
*imported* binding is illegal TS, so cross-module mutations always occur in
the defining module's functions.

## First-abort loop after uniqueId

With uniqueId cleared, `smelt build` at the pinned es-toolkit root advances
to further pre-existing families (verified pre-existing by re-running main's
binary, which still aborts at uniqueId.ts first). Fixed generally here:

1. **Narrowing leak across sibling tests** (`isEqualWith.spec.ts`): an
   observed-type narrowing recorded in one `it` body
   (`array1 = /c/.exec(...)` → `Optional<SmeltMatch>`) leaked into a sibling
   `it` declaring its own `array1`, turning its indexed write into a
   non-assignable `OptionalIndex`. Narrowing facts are now scoped per test
   case exactly like `self.locals`
   (`lowering/testing/suites.rs`).
2. **`delete list[i]`** (compat `unset` specialized over lists): dense
   `Vec<T>` lists cannot represent holes, so the delete emits a successful
   no-op `true` — the same explicit-deferral style as no-op list `length`
   growth (`emitter/map.rs`).
3. **List-extend coercion** (`emitter/list_mutation.rs`): a generic
   `SmeltList<T>` argument extending an erased `SmeltList<SmeltUnknown>`
   receiver now coerces through the shared `value_at_type` conversion instead
   of erroring.
4. **String-contains erased haystack** (`emitter/strings.rs`): unscoped
   type-parameter/union/`never`/erased-class haystacks route through the same
   runtime `.includes` boundary as plain `Unknown`; the positional form
   coerces via `value_at_type`. Several emit errors in this loop also now
   include the offending types in their messages.

## Status / remaining families

- The pinned es-toolkit root still does **not** emit `dist-smelt`. The abort
  order after this work (first-error at a time, enumerated in a scratch copy
  with per-family excludes):
  1. **`globalThis.File = class …` / `global.Buffer = …` monkey-patching**
     (`isBlob.spec.ts`, `isFile.spec.ts`, `isBuffer.spec.ts`) — the
     documented dynamic non-goal (global-object member writes); these specs
     are exclusion candidates like the DOM specs, or need a modeled
     global-property store. This is now the first abort at the pinned root.
  2. **Optional-chained methods on modeled collection receivers**
     (`stack?.has(source)` / `stack?.set(...)` on `Map | undefined` in
     `compat/predicate/isMatchWith.ts`): the stdlib method dispatch handles
     the non-optional receiver; the optional form falls through to the
     generic `OptionalMethod`, whose codegen has no JS-Map surface. Needs an
     optional-receiver desugar in the stdlib dispatch (architectural, not a
     one-line arm).
  3. Unknown further tail behind (2) — enumeration stopped there.
- `delete array[i]` sparse-hole *runtime semantics* remain deferred (the
  no-op transpiles; hole observations diverge at runtime).

## Verification

- Frontend: `crates/smelt-frontend-ts/src/tests/module_globals_tests.rs`
  (lift + GlobalGet/GlobalSet shapes, prefix/postfix value semantics,
  compound reads, `var`, non-mutated inline path unchanged, local shadowing,
  both named blockers, narrowing scope regression).
- Codegen: `crates/smelt-codegen-rust/src/tests/module_globals_tests.rs`
  (const-init `Cell` and `RefCell<String>` thread-locals, get/set text,
  gating with no globals, delete-on-list no-op).
- End-to-end fixture mirroring uniqueId (module counter,
  `uniqueId(prefix?)` returning `` `${prefix}${++counter}` ``, spec asserting
  increments across calls in one test): `smelt build` + generated
  `cargo test` green.
- `cargo check --workspace --exclude smelt-gui`: clean.
- `cargo clippy --lib -W clippy::pedantic` on smelt-hir, smelt-mir,
  smelt-codegen-rust, smelt-frontend-ts, smelt-frontend-py,
  smelt-transpiler: clean.
- `cargo test --workspace --exclude smelt-gui`: 1755 passed, 0 failed.
- No net `SmeltUnknown` increase: the new expression kinds and cells are
  fully concrete (primitive `Cell`/`RefCell` ABI); the string-contains /
  list-extend changes reuse existing erased boundaries for values that were
  already erased.
