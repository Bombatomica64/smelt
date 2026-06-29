# TypeScript Global Objects Plan

Smelt should support TypeScript global objects as real JavaScript objects in
generated Rust when runtime identity is observable. Compile-time erasure is also
required for probes whose result is fully known and whose global object value
does not escape. Code that reads, aliases, mutates, or passes `globalThis`,
`global`, `self`, and Node ambient globals should continue to have object
identity and a useful static shape whenever TypeScript gives Smelt one.

Window and browser DOM behavior are out of scope for the current feature phase.
The initial target is non-DOM TypeScript libraries such as `es-toolkit`, where
the important globals are ECMAScript intrinsics, `globalThis`, Node-compatible
ambient surfaces, and library-level feature probes.

## Goals

- Lower `globalThis` as a concrete object value, not as erased feature-detection
  syntax, whenever the value is observed at runtime.
- Erase global feature probes at compile time when doing so preserves JavaScript
  behavior and leaves no observable global object identity.
- Avoid creating the runtime global object in generated Rust when every global
  use in the module graph was erased or normalized before codegen.
- Preserve global object identity across aliases, property reads, property
  writes, computed access, and `"key" in globalThis` checks.
- Keep useful static shapes for known global members such as `Array`, `Object`,
  `Symbol`, `Map`, `Set`, `Reflect`, `Promise`, `JSON`, `Math`, `Date`,
  `RegExp`, `structuredClone`, `process`, `console`, and `crypto`.
- Route dynamic or user-created properties through an explicit object property
  store instead of making ordinary global access become `SmeltUnknown`.
- Keep existing specialized stdlib lowering usable through `globalThis.X` and
  aliases to the global object.
- Make runtime availability configurable by target profile while keeping the
  default generated Rust test profile deterministic.

## Non-Goals

- Do not implement DOM `window` APIs in this phase.
- Do not special-case every package probe as an isolated lowering rule.
- Do not make `SmeltUnknown` the default internal ABI for global values that
  still have known static shape.
- Do not replace generated Rust incremental behavior with unconditional writes
  while iterating on generated crates.

## Design

### 1. Add a Runtime Global Object

Create a runtime-backed global object in `smelt-runtime` and expose it from
generated crates only when a post-erasure usage analysis proves it is needed.

The runtime type should model three pieces separately:

- Stable identity, so aliases to `globalThis` observe the same object.
- Known slots, for statically modeled globals and constructors.
- Dynamic slots, for user-defined or computed properties.

The first implementation can use interior mutability behind a shared handle:

```rust
pub struct SmeltGlobalObject {
    id: usize,
    slots: std::cell::RefCell<SmeltRecord<String, SmeltUnknown>>,
}

pub type SmeltGlobal = std::rc::Rc<SmeltGlobalObject>;
```

If generated code later needs `Send` or async cross-thread behavior, move the
backing store to `Arc<RwLock<_>>`. Do not start there unless the generated code
actually needs it.

Generated modules should obtain the ambient object through a single helper such
as `smelt_global_this()`. The helper must return the same object for the same
generated program execution, not a fresh record per read. The emitter should
only include this helper when the lowered HIR/MIR still contains a real global
object operation after compile-time erasure has run.

### 2. Add Compile-Time Global Erasure

Add an erasure pass before runtime-global lowering or codegen. It should fold
global probes and normalize known static paths that do not require object
identity:

- `typeof globalThis !== "undefined"` -> `true`
- `typeof globalThis === "object"` -> `true`
- `"Map" in globalThis` -> `true` when `Map` is present in the target profile
- `"window" in globalThis` -> `false` for the current non-DOM profile
- `globalThis.Object.keys(value)` -> `Object.keys(value)` when `globalThis` is
  used only as a namespace receiver

This pass must run before the decision to emit the runtime global object. If all
global usages disappear during erasure or namespace normalization, generated
Rust should not allocate or initialize a global object.

Erasure must be conservative. Do not erase when any of these are true:

- The global object is assigned to a local that escapes or is returned.
- A dynamic key is read or written.
- A property write targets the global object.
- The code compares global object identity.
- The value is passed to a function that could inspect or mutate it.

### 3. Add HIR/MIR Operations for Global Object Access

Avoid encoding global object behavior as ordinary dictionary access alone. A
global object has known slots plus a dynamic property bag, and codegen should be
able to preserve typed paths.

Add focused operations along these lines:

- `ExprKind::GlobalObject`
- `ExprKind::GlobalMember { object, member }`
- `ExprKind::GlobalComputedMember { object, key }`
- `ExprKind::GlobalSet { object, key, value }`
- `ExprKind::GlobalContainsKey { object, key }`

MIR can lower these to equivalent `Rvalue` operations. Static known members keep
their concrete type; dynamic reads return `SmeltUnknown` unless a narrowed or
declared shape is available.

This keeps `globalThis.Map` equivalent to `Map`, while
`globalThis[someKey]` remains a real runtime object lookup.

### 4. Add a Global Shape Registry

Move ambient-global knowledge out of scattered lowering helpers and into a
small focused module, for example
`crates/smelt-frontend-ts/src/lowering/ambient_globals.rs`.

The registry should answer:

- Which identifiers are global object aliases for the active target:
  `globalThis`, `global`, `self`; `window` only as an absent/non-DOM alias for
  this phase.
- Which static members have known types.
- Which static members map to existing stdlib rules.
- Which static members are runtime capability slots.
- Which ambient objects are nested objects, such as `process.env`.

This module should not lower expressions itself at first. It should provide
classification APIs used by the erasure pass, `static_member`,
`computed_member`, call lowering, binary `in`, `typeof`, and assignment
lowering.

### 5. Preserve Existing Stdlib Lowering Through Global Paths

The lowering dispatch currently recognizes many calls by callee spelling, such
as `Object.keys(...)`, `Array.isArray(...)`, `JSON.parse(...)`, `Math.max(...)`,
and `process.cwd()`.

Extend dispatch to normalize global paths before rule lookup:

- `globalThis.Object.keys(x)` -> `Object.keys(x)`
- `const g = globalThis; g.Object.keys(x)` -> `Object.keys(x)` when the alias is
  known to be the global object.
- `globalThis.process.cwd()` -> the same Node surface as `process.cwd()`.

This is a path normalization layer, not a package-specific exception. When the
normalized path consumes the only global usage, the runtime global object should
remain uncreated.

### 6. Support Aliasing and Mutation

Track local bindings that are aliases of the global object:

```ts
const g = globalThis;
g.__feature = true;
globalThis.__feature === true;
```

This requires a local alias table in TS lowering and a runtime shared object in
generated Rust. Static alias tracking should only be used for preserving known
member types and stdlib dispatch. Correctness for dynamic properties comes from
the shared runtime object.

Assignments to known readonly global slots should either be rejected with a
clear unsupported diagnostic or routed through the dynamic store only when
JavaScript semantics allow shadowing. For this phase, prefer explicit
diagnostics for writes to intrinsic constructors, and implement dynamic writes
for non-reserved keys.

### 7. Target Profiles

Introduce a small target profile object, with the default generated Rust test
profile set to a deterministic non-DOM, Node-compatible environment:

- `globalThis`: present
- `global`: present and aliased to `globalThis`
- `self`: absent or aliased only when a worker-like profile is selected
- `window`: absent for now
- `process`: present
- ECMAScript intrinsics: present
- `structuredClone`: present if runtime support exists
- `crypto`: present only for the currently supported non-secure deterministic
  surfaces such as `getRandomValues`

This is where feature detection and erasure should be answered. The important
point is that compile-time answers and runtime global object shape must come
from the same profile table, so erased probes and non-erased runtime object
reads cannot disagree.

### 8. Generated Rust Runtime Surface

Generated crates should emit or import helper functions for:

- Creating the global object once.
- Reading known global slots.
- Reading and writing dynamic slots.
- Testing property presence.
- Converting global slots to and from `SmeltUnknown` at real dynamic
  boundaries.

Keep the helper implementation in `smelt-runtime` where possible. If generated
crates currently inline runtime support, add the minimal shim there first and
move it into `smelt-runtime` in a follow-up only if that matches the existing
emitter structure.

The emitter must perform the runtime-global-needed check after erasure and
normalization. A source module that only contains erased probes such as
`typeof globalThis` or normalized namespace access such as
`globalThis.Object.keys(value)` should not emit global object initialization.

## Implementation Phases

### Phase 1: Frontend Shape and Tests

- Add the ambient global registry module with docstrings on all public helper
  functions.
- Add compile-time erasure for known feature probes and namespace-only global
  paths.
- Add TS frontend tests for:
  - `globalThis` expression lowering.
  - `global` aliasing `globalThis`.
  - `typeof globalThis`.
  - `"Map" in globalThis`.
  - `globalThis.Object.keys`.
  - `const g = globalThis; g.Array.isArray(value)`.
  - Dynamic property read/write through a global alias.
- Add negative tests proving erasure does not run when global identity,
  dynamic access, or mutation is observable.
- Keep `window` as absent in the non-DOM profile.

### Phase 2: HIR/MIR Global Operations

- Add global-object expression and property operations to HIR and MIR.
- Format the new operations in HIR/MIR formatters.
- Lower static known members with concrete types.
- Lower dynamic reads and writes through explicit global operations.
- Add focused unit tests before generated-crate tests.

### Phase 3: Rust Codegen Runtime

- Add emitted/imported Rust helpers for `SmeltGlobal`.
- Add a post-erasure usage check so generated crates only initialize the global
  object when real global operations remain.
- Preserve object identity across aliases.
- Implement `in`, static member reads, computed member reads, and dynamic writes.
- Ensure known constructors and namespaces still dispatch to the existing
  generated Rust stdlib paths.

### Phase 4: es-toolkit Compatibility Pass

- Generate the es-toolkit crate.
- Use `smelt rust-test-report` for runtime compatibility investigation.
- Write reports to `blocker-logs/<name>.md`.
- Fix the largest global-object failure families first.
- Avoid broad codebase refactors until the generated tests stabilize.

### Phase 5: Cleanup and Target Profiles

- Move any remaining `process.*`, `typeof window`, and `crypto.*` ad hoc hooks
  behind the ambient global registry.
- Add profile configuration only after the default non-DOM Node-compatible
  profile works.
- Document target-profile behavior in user-facing compatibility docs.

## Acceptance Criteria

- `globalThis`, `global`, and aliases lower as real object values.
- Compile-time-only probes and namespace-only global paths erase before codegen.
- Generated Rust does not create the runtime global object when all global uses
  were erased or normalized.
- Reads and writes through aliases observe shared object identity in generated
  Rust tests.
- Known global members retain concrete static types.
- Existing direct stdlib calls and equivalent `globalThis.*` calls emit the same
  behavior.
- Dynamic global properties use explicit `SmeltUnknown` boundary adapters only
  at dynamic reads/writes.
- `cargo check` and `cargo clippy` pass before merging implementation PRs.
- Full `cargo test` is run before committing implementation phases.
