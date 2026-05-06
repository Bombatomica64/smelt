# Smelt Max TypeScript Before Python Checklist

## Status

- [x] Phase 0 baseline checked on current checkout.
- [x] Phase 1 TypeScript sync expression support.
- [x] Phase 2 mutation and loops.
- [x] Phase 3 classes, interfaces, constructors, methods.
- [x] Phase 4 imports and multi-file TypeScript.
- [x] Phase 5 async, await, and Promise lowering.
- [ ] Phase 6 standard library mapping.
- [ ] Phase 7 Express prep slice.

## Phase 0: Stability

- [x] Confirmed no pending stabilization work was present in this checkout.
- [x] Ran `cargo fmt --check`.
- [x] Ran `cargo test -q`.
- [x] Committed Phase 1 as `de44e6b Expand TypeScript sync expression support`.

## Phase 1: TypeScript Sync Core Expressions

- [x] Array literals.
- [x] Tuple literals with tuple annotation.
- [x] Record object literals with `Record<string, T>` annotation.
- [x] Index expressions.
- [x] Static member expressions for record field reads.
- [x] Unary `!` and `-`.
- [x] Logical `&&` and `||` as direct boolean rvalues.
- [x] General supported call arguments.
- [x] Type mapping for `number[]`, `Array<T>`, tuple types, `Record<string, T>`, and `T | null/undefined`.
- [x] MIR lowering for list, dict, tuple, index, field, unary, and logical expressions.
- [x] Rust codegen for list, dict, tuple, index, field, unary, and aggregate `console.log`.
- [x] End-to-end fixtures `06` through `11`.

## Phase 2: Mutation And Loops

- [x] Add shared assignment representation.
- [x] Add MIR place assignment support.
- [x] Expand MIR places for field and index assignment.
- [x] TypeScript support for `x = expr`.
- [x] TypeScript support for compound assignments.
- [x] TypeScript support for statement-only increment/decrement.
- [x] TypeScript support for `while`.
- [x] TypeScript support for `for...of`.
- [x] TypeScript support for C-style `for`.
- [x] TypeScript support for `break` and `continue` inside loops.
- [x] Switch with `break`, still rejecting fallthrough.
- [x] CFG lowering for `while`.
- [x] Loop context stack for `break` and `continue`.
- [x] Rust codegen for structured `while`.
- [x] Rust codegen for field and index assignment places.
- [x] Fixtures `12` through `17`.
- [x] Added fixtures `12_while_sum` and `14_c_for_loop`.
- [x] Added fixtures `13_for_of_sum`, `15_break_continue`, `16_switch_break_no_fallthrough`, and `17_mutating_array`.
- [x] `cargo fmt --check`.
- [x] `cargo test -q`.
- [x] Committed partial Phase 2 slice as `Add TypeScript mutation and while loop support`.

Current Phase 2 status: complete on the current checkout. The integration suite covers fixtures
`12` through `17`, including nested loop control flow, switch cases with explicit breaks, `for...of`,
and mutating array assignments.

## Phase 3: TypeScript Object Model

- [x] Added HIR item model for classes, interfaces, visibility, class kind, and function ownership.
- [x] Propagated class kind, base, fields, constructor, methods, interfaces, and visibility through MIR.
- [x] TypeScript support for class declarations with fields.
- [x] TypeScript support for constructor lowering and `new Class(...)`.
- [x] TypeScript support for `this` as a method/constructor local.
- [x] TypeScript support for field reads and writes on class values.
- [x] TypeScript support for sync method declarations and method calls.
- [x] TypeScript support for mutating methods.
- [x] TypeScript support for interface field declarations.
- [x] TypeScript support for interface method signatures.
- [x] TypeScript validation for implemented interface fields and method signatures.
- [x] TypeScript captures private/protected field visibility as metadata.
- [x] Rust codegen emits class structs and impl blocks for constructors and methods.
- [x] Fixtures `18` through `25`.
- [x] Decide whether single inheritance is considered complete for Phase 3 or remains metadata-only.
- [x] Add/confirm negative coverage for unsupported object-model features: optional fields, computed fields, generic classes/interfaces, static members, getters/setters, decorators, and abstract classes.
- [x] Object-model v1 follow-up:
  - [x] Defer TS/Py optional fields as `Option<T>` / `T | None` until explicit construction semantics are designed.
    - [x] TS optional interface fields participate in shape checks.
  - [x] Support TS interface inheritance by flattening inherited requirements before shape checks.
  - [x] Support literal computed TS property names; reject dynamic computed names.
  - [x] Defer static methods and static constants/class vars until associated/module item lowering is designed.
  - [x] Defer abstract classes/methods beyond current frontend rejection until enforcement semantics are designed.
  - [x] Defer getter/setter and `@property` sugar until method-sugar lowering is designed.
  - [x] Decide single-inheritance lowering: metadata-only for now.
  - [x] Defer generic classes/interfaces unless monomorphization becomes the active milestone.
- [x] Re-run full verification after the remaining Phase 3 cleanup.

Current Phase 3 status: complete for the current v1 scope. The completed slice covers the
class/interface core path end-to-end, including generated Rust for constructors, methods, field
access, and interface implementation checks. Single inheritance remains metadata-only, and the
unsupported object-model features are covered by frontend rejection tests or explicitly deferred.

## Phase 4: Module Linking And LSP Stubs

- [x] Added path-aware TypeScript lowering while preserving the historical `main` module name.
- [x] TypeScript import declarations are captured in HIR import metadata.
- [x] TypeScript named/default/namespace imports create local aliases for already-lowered items.
- [x] Added path-aware Python lowering while preserving the historical `main` module name.
- [x] Python `import` and `from ... import ...` statements are captured in HIR import metadata.
- [x] Python imported names create local aliases for already-lowered items.
- [x] Manifest `check` lowers TypeScript and Python entries and emits LSP declaration stubs.
- [x] Build/check paths emit both `.d.ts` and `.pyi` files for every lowered entry.
- [x] Added CLI coverage for linked TypeScript modules and generated `.d.ts`/`.pyi` files.
- [x] Added CLI coverage for linked Python modules and generated `.d.ts`/`.pyi` files.
- [x] Manifest lowering shares one HIR crate across TypeScript and Python frontends in entry order.
- [x] Added CLI coverage for a Python entry importing and running a TypeScript function.
- [x] `just try-modules` builds and runs a Python entry that imports a TypeScript function.
- [x] Add order-independent module graph resolution instead of relying on manifest order.
  - [x] Scan TypeScript and Python import declarations before lowering.
  - [x] Resolve import specifiers to manifest entries across `.ts` and `.py` files.
  - [x] Lower manifest entries in dependency order.
  - [x] Add CLI coverage for importers listed before dependencies.
- [x] Add import path canonicalization for package roots, index modules, and Python package directories.
  - [x] Resolve extensionless `.ts` / `.py` manifest imports.
  - [x] Resolve TypeScript package-style `index.ts` imports.
  - [x] Add CLI coverage for Python package directory `__init__.py` imports.
- [x] Expand mixed-language linking beyond manifest-order item imports.

Current Phase 4 status: complete for the current v1 scope. Manifest entries can reference items
lowered from local dependencies across TypeScript and Python, even when manifest entries list
importers before dependencies. Package-style TypeScript `index.ts`, Python package `__init__.py`,
and both Python-to-TypeScript and TypeScript-to-Python function imports have CLI coverage.

## Phase 5: Async Model

- [x] Add async metadata to HIR functions and methods.
- [x] Represent async bodies in HIR as an explicit state machine so frontends can lower `await`
      without baking in a specific backend runtime too early.
- [x] Lower TypeScript `async` functions into the HIR async state-machine model.
- [x] Lower TypeScript `await` into state-machine suspension points.
- [x] Map TypeScript `Promise<T>` to the HIR async result/future representation.
- [x] Map common TypeScript promise combinators:
  - [x] `Promise.all`.
  - [x] `Promise.race`.
  - [x] `Promise.allSettled`.
- [x] Add TypeScript timer/platform shims only where needed by fixtures, keeping the core
      `Promise`/`await` lowering independent of those APIs.
- [x] Lower Python `async def` and `await` into the same HIR async state-machine model.
- [x] Create a separate crate for Python `asyncio` transformations.
  - [x] Keep the crate responsible only for recognizing and rewriting `asyncio` APIs.
  - [x] Rewrite into Smelt runtime abstractions instead of directly depending on Tokio/Axum
        concepts from the Python frontend.
  - [x] Preserve the option to swap the runtime backend later, even if Tokio/Axum remains the
        expected target.
- [x] Add Python `asyncio` rewrite coverage for:
  - [x] `asyncio.create_task`.
  - [x] `asyncio.gather`.
  - [x] `asyncio.sleep`.
  - [x] `asyncio.wait_for`.
  - [x] `asyncio.Queue`.
  - [x] `asyncio.Lock`.
- [x] Reject or explicitly mark unsupported lower-level event-loop APIs in the first async slice:
      `get_event_loop`, `get_running_loop`, `call_soon`, custom futures, transports, and
      protocols.
- [x] Define cancellation semantics for HIR async tasks.
- [x] Add MIR lowering for async state machines.
- [x] Add Rust codegen for async state machines and runtime-backed task operations.
  - [x] Emit basic Rust `async fn` and `.await` for TypeScript async/await.
  - [x] Add runtime-backed task operations.
- [x] Add fixtures covering TS async/await, TS promise joins, Python async/await, and rewritten
      `asyncio` APIs.
  - [x] Add TypeScript async/await HIR examples.

Current Phase 5 status: complete on the current checkout. TypeScript and Python async syntax lower
into shared HIR async state-machine metadata, MIR async functions, await rvalues, and runtime async
ops. TypeScript promise joins, the minimal timer shim, and Python `asyncio` calls lower through
runtime-neutral HIR/MIR operations; Rust codegen maps those operations to Tokio. Cancellation
semantics for this slice follow Rust/Tokio drop semantics: dropping a future cancels pending work,
`wait_for`/timeout drops the timed-out future, and spawned task wrappers surface task panics as
runtime errors.

## Phase 6: Standard Library Mapping

- [ ] Prefer direct semantic transpilation for stdlib operations whenever the Rust equivalent is
      straightforward and preserves source-language behavior.
  - [ ] Use a typed mapping registry instead of ad-hoc string matches:
        `(language, receiver/function, receiver type, argument shape) -> lowering rule`.
  - [ ] Keep rules able to lower to inline Rust/MIR operations before considering runtime helpers.
- [ ] Keep `smelt-runtime` as a last-resort compatibility layer, not the default stdlib strategy.
  - [ ] Runtime helpers must justify behavior that cannot stay readable or correct as inline Rust.
  - [ ] Prefer external Rust crates with generated dependency injection over custom runtime shims.
- [ ] Support native/C-backed integrations as explicit library mappings, not a general CPython
      extension fallback.
  - [ ] Only call native APIs with stable C ABI boundaries and clear ownership/error semantics.
  - [ ] Do not treat arbitrary `PyObject*`/CPython-extension APIs as "direct C" without an explicit
        hybrid backend decision.
  - [ ] Make NumPy the first required Python native-library target because too much practical Python
        depends on it.
  - [ ] Design NumPy around array/tensor semantics first, then choose whether each operation maps to
        Rust-native arrays/crates, C ABI calls, or a deliberately hybrid path.
- [x] First direct-transpile stdlib slice:
  - [x] TypeScript `Array.prototype.length`.
  - [x] TypeScript `String.prototype.length`.
  - [x] Python `len(...)` for list/dict/tuple/string values.
- [ ] Follow-up direct mappings:
  - [ ] TypeScript array methods: `map`, `filter`, `reduce`, `forEach`, `find`, `includes`, `push`,
        and `slice`.
    - [x] TypeScript array `includes`.
  - [ ] TypeScript string methods: `split`, `toLowerCase`, and `toUpperCase`.
    - [x] TypeScript `toLowerCase` and `toUpperCase`.
    - [x] Python `str.lower()` and `str.upper()`.
    - [x] TypeScript string `includes`.
    - [x] Python string `in` / `not in`.
    - [x] TypeScript string `split`.
    - [x] Python `str.split(separator)`.
  - [ ] TypeScript `Math.*`, `Object.*`, `JSON.*`, `fetch`, and async timers.
  - [ ] Python builtins and common stdlib functions that map directly to Rust or external crates.
    - [x] Python list `in` / `not in`.

## Phase 6/TypeScript Next Coverage

- [ ] Closures with captures.
- [ ] Generic functions/classes beyond trivial cases.
- [ ] Union and discriminated-union modeling beyond nullish optionals.
- [ ] Maps, Sets, and iterable support.
  - [x] String index access and `for...of` over strings.
- [ ] Type narrowing/control-flow type analysis.
- [ ] Object spread/rest/destructuring.
- [ ] Callback-heavy stdlib methods.

## Later Phases

- [ ] Finish remaining Phase 3 object model cleanup.
- [ ] Finish remaining Phase 4 module linking cleanup.
- [x] Phase 5 async model.
- [ ] Phase 6 stdlib mapping.
- [ ] Phase 7 Express recognizer and Axum codegen path.
