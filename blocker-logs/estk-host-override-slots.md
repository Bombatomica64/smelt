# es-toolkit blocker: host-global override slots

## Family

Whole-global reassignment of modeled host constructors:

```ts
const originalFile = globalThis.File;
globalThis.File = class File extends Blob { /* ... */ };   // override with a ctor
globalThis.Blob = undefined;                               // mark absent
globalThis.File = originalFile;                            // restore native
```

This is the pattern in `src/predicate/isBlob.spec.ts` and `src/predicate/isFile.spec.ts`
(pinned `e008a281`). On `main` the whole-crate `smelt build` aborted here:

```
MIR lowering failed: only local, field, and index expressions can be assigned
  at src/predicate/isBlob.spec.ts   (the `globalThis.File = ...` target)
```

Full monkey-patching (prototype/method patching) remains a non-goal and is still rejected.

## Design (as implemented)

Bounded, **pay-for-use** support gated per host name, only for names the crate writes
somewhere (`globalThis.<Name> =`). Unwritten names keep byte-identical presence folding and
native construction (regression-tested).

### Runtime model (codegen prelude, gated on any written name)

```rust
#[derive(Clone)] enum SmeltHostOverride { Native, Absent, Ctor(SmeltUnknown) }
thread_local! {
    static SMELT_HOST_OVERRIDE_<NAME>: RefCell<SmeltHostOverride> =
        const { RefCell::new(SmeltHostOverride::Native) };
}
```

One slot per written name (fresh `Native` per test thread — matches the specs' save/restore
discipline). Three fixed helpers named in `smelt-stdlib/src/runtime_symbols.rs`
(`host_override` module):

- `smelt_host_override_read(slot, name)` — `Native` yields a native-handle marker record
  `{ "__smelt_native_ctor": true, "name": "<Name>" }` (an identity token for save/restore, not
  a callable); `Ctor(v)` yields the stored ctor; `Absent` yields `undefined`.
- `smelt_host_override_write(slot, value)` — classifies the stored value: `undefined` → `Absent`;
  a native-handle marker → `Native`; a function/class ctor → `Ctor`. Returns the stored value.
- `smelt_host_override_present(slot)` — `false` only for `Absent`.

### HIR / MIR

Three new `ExprKind`s mirrored as MIR `Rvalue`s (DateFromParts-precedent plumbing), with
exhaustive-match arms everywhere (map.rs mapper, formatters, validators, opt, lower/place):
`HostGlobalRead { class }`, `HostGlobalWrite { class, value }` (evaluates to the stored value),
`HostGlobalPresent { class }` (bool). No new `Place` variant — `globalThis.X = v` lowers in the
frontend assignment path to `HostGlobalWrite`, never through `lower_place`.

### Frontend

- **Crate-level pre-pass** (`scan_written_host_globals`, `HirCtx::written_host_globals`) records
  which modeled host names have `globalThis.<Name> =` writes *anywhere*, before any module
  lowers — so a write in a spec activates the machinery in the predicate module that lowers
  first. Runs in the manifest build, the probe pass, and single-file `dump-hir`/`dump-mir`.
- **Written names only**: presence guards (`typeof X === 'undefined'`, both operand orders, bare
  and `globalThis.X` member forms) lower to `HostGlobalPresent` instead of folding; `new X(...)`
  lowers to a slot-checked dispatch (`typeof slot === 'function' ? closure-call stored ctor :
  native construction` — both paths emitted); a class-expression override value lowers to a
  constructor closure so the slot holds a `Function`; bare `globalThis.X` reads lower to
  `HostGlobalRead`. Unwritten names: every existing path unchanged.
- **Save/restore round trip**: `const o = globalThis.File; …; globalThis.File = o` returns the
  slot to `Native` via native-handle classification.
- **v1 named blockers** (never `SmeltUnknown`-to-compile): a write of a value other than
  `undefined` / a class-or-function ctor / a saved native handle is a named error; a compound
  assignment to a host global is a named error; writes to unmodeled `globalThis` members keep
  today's behavior.

### instanceof soundness (host subclass)

`isBlob(new File(...)) === true` for an override `class File extends Blob` is made sound by a
**general** rule, not a globalThis special-case: a class whose single-inheritance base chain
reaches a modeled host object carries that host's identity marker(s) when erased to
`SmeltUnknown` (`class_unknown_object_text` → `host_base_markers`). `File` additionally carries
`Blob`'s marker (the host subtype relationship the native `new File(...)` records already stamp).
So `value instanceof Blob` (a marker check on the erased value) is honest for host subclasses.

## Result

- `dump-mir` on `isBlob.spec.ts` and `isFile.spec.ts`: both lower cleanly; the assignment family
  is gone (`host_global_read/write/present` rvalues present, no MIR error).
- Whole-crate `smelt build` at the es-toolkit root:
  - **before** (main): abort at `src/predicate/isBlob.spec.ts` — "only local, field, and index
    expressions can be assigned" (the `globalThis.File` assignment family).
  - **after**: abort moves to the next family — optional-field codegen for a `SmeltJsMap`
    receiver (`.has`), i.e. optional-chained methods on modeled Map receivers, which belongs to a
    sibling worktree. My family no longer aborts.
- e2e fixture (`scratchpad/estk-host-override-fixture`, three scenarios mirroring
  `isBlob.spec.ts`): `smelt build` + generated `cargo test` green (2/2).

## Documented gaps

- The override-class construction drops the host base `super(...)` call (Smelt's existing
  host-base model), so an override `File` instance carries the `Blob` marker for `instanceof`
  but not the underlying blob byte payload. This is sufficient for the `isBlob`/`isFile`
  predicates (identity only); reads of `.size`/`.type` on an *override*-constructed instance are
  not modeled. Native `new File(...)`/`new Blob(...)` (slot `Native`) retain full field fidelity.
- The `new X(...)` dispatch emits both the ctor-call and native-construction branches, so their
  argument expressions are lowered twice. Harmless for the literal args these specs use; a
  future refactor could hoist shared args.
