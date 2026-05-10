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
- [x] Static TypeScript tuple indexing.
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
- [ ] Fast-port execution plan:
  - [ ] Batch ports by HIR/MIR shape, not by source language, so one MIR/codegen operation can
        unlock multiple TypeScript and Python APIs.
  - [ ] Prefer leaf expression mappings first: no callbacks, no mutation, no external crates, no
        control-flow rewrites.
  - [ ] Add one shared HIR expression/rvalue per semantic operation, then wire every source API that
        maps to it in the same slice.
  - [ ] For each slice, add frontend unit tests, MIR/codegen tests, and one end-to-end fixture only
        when stdout/runtime behavior matters.
  - [ ] Keep generated Rust boring and direct; only introduce runtime helpers when the inline Rust
        would be wrong, unreadable, or repeatedly duplicated.
- [ ] Batch A, scalar/string/list leaf mappings with no new dependencies:
  - [x] String prefix/suffix checks:
    - [x] TypeScript `startsWith` and `endsWith`.
    - [x] Python `str.startswith()` and `str.endswith()`.
  - [x] String trim variants:
    - [x] TypeScript `trimStart` and `trimEnd`.
    - [x] Python `str.lstrip()` and `str.rstrip()` with no arguments.
  - [x] String search:
    - [x] TypeScript `indexOf` and `lastIndexOf`.
    - [x] Python `str.find()` and `str.rfind()`.
    - [x] Current Rust output returns Rust byte offsets; Unicode user-index parity is deferred.
  - [x] String replace literal:
    - [x] TypeScript `replace` for literal string pattern.
    - [x] Python `str.replace()` for string pattern/replacement.
  - [x] Numeric predicates and math:
    - [x] TypeScript `Math.trunc`, `Math.sign`, `Math.sqrt`, `Math.pow`, `Math.max`, and `Math.min`.
    - [x] Python `math.trunc`, `math.sqrt`, `math.pow`, `max(...)`, and `min(...)`.
    - [x] TypeScript `Math.sign` currently lowers to Rust `signum()` for normal finite numbers; JS `-0`/`NaN` edge parity is deferred.
  - [x] Collection contains parity:
    - [x] Python tuple `in` / `not in`.
    - [x] Python dict key `in` / `not in`.
- [ ] Batch B, mutation methods with existing place-assignment support:
  - [x] TypeScript `Array.prototype.push`.
  - [x] TypeScript `Array.prototype.pop`.
  - [x] TypeScript `Array.prototype.reverse`.
  - [x] Python `list.append`.
  - [x] Python `list.pop`.
  - [x] Python `list.remove`.
  - [x] Python `list.clear`.
  - [x] Python `list.reverse`.
  - [x] Python `list.sort`.
  - [x] Python `list.insert`.
  - [x] Python `dict.update`.
  - [x] Python `dict.pop`.
  - [x] Python `dict.clear`.
- [ ] Batch C, collection projection methods:
  - [x] TypeScript `Object.keys`, `Object.values`, and `Object.entries`.
  - [x] Python `dict.keys`, `dict.values`, and `dict.items`.
  - [x] TypeScript `Array.prototype.join`.
  - [x] Python `str.join`.
  - [x] TypeScript `Array.prototype.concat`.
  - [x] Python `list.extend`.
- [ ] Batch D, slicing/indexing semantics:
  - [x] TypeScript `Array.prototype.slice` with omitted and positive indexes.
  - [x] TypeScript `String.prototype.slice` with omitted and positive indexes.
  - [x] Python list slicing with positive indexes.
  - [x] Python tuple slicing with positive indexes.
  - [x] Python string slicing with positive indexes.
  - [x] Shared slice-bound policy: HIR/MIR uses Python-style negative-index normalization; TypeScript `slice` lowers to that shared semantic because supported JS slice bounds match it.
  - [x] Shared element-index policy: HIR/MIR uses Python-style negative-index normalization for list/string element access; TypeScript `.at(...)` lowers to it, while negative bracket indexes are rejected because JavaScript treats them as property lookups.
- [ ] Batch E, callback-heavy methods after closure support:
  - [x] TypeScript `Array.prototype.map` with capture-free expression callbacks.
  - [x] TypeScript `Array.prototype.filter` with capture-free expression callbacks.
  - [x] TypeScript `Array.prototype.reduce` with explicit initial value and capture-free expression callback.
  - [x] TypeScript `Array.prototype.forEach` with capture-free expression callbacks.
  - [x] TypeScript `Array.prototype.find` and `findIndex` with capture-free expression callbacks.
  - [x] TypeScript `Array.prototype.some` and `every` with capture-free expression callbacks.
  - [ ] TypeScript callback methods with captured closures, callback function values, and index/array parameters.
  - [ ] Python `map`, `filter`, and callback-style `sorted(key=...)` if supported.
- [ ] Batch F, dependency-backed mappings:
  - [ ] `serde_json` dependency injection, then TypeScript `JSON.stringify` / `JSON.parse` and
        Python `json.dumps` / `json.loads`.
    - [x] `serde_json` dependency injection.
    - [x] TypeScript `JSON.stringify`.
    - [x] TypeScript `JSON.parse<T>` for JSON-compatible explicit target types.
    - [x] Python `json.dumps`.
    - [x] Python `json.loads` for annotated JSON-compatible destination types.
  - [ ] `regex` dependency injection, then regex-backed TypeScript `replace` / `replaceAll` and
        Python `re` basics.
    - [x] `regex` dependency injection for generated Rust crates.
    - [x] TypeScript `new RegExp(pattern).test(text)` boolean matching.
    - [x] Python `re.search`, `re.match`, and `re.fullmatch` boolean matching.
    - [ ] Regex replacement, splitting, captures, flags, and compiled regex values.
  - [x] `reqwest` dependency injection, then TypeScript `fetch` and Python HTTP mapping decision.
    - [x] Python `requests.get(url)` maps to blocking Reqwest response text.
  - [ ] `chrono` dependency injection, then TypeScript `Date` and Python `datetime` decision.
  - [ ] RNG dependency/policy, then TypeScript `Math.random` and Python `random` basics.
    - [x] TypeScript `Math.random`.
    - [x] Python `random.random`.
- [ ] Batch G, native/data libraries:
  - [ ] NumPy scalar dtype model.
  - [ ] NumPy one-dimensional array construction and indexing.
  - [ ] NumPy shape/size/ndim metadata.
  - [ ] NumPy elementwise arithmetic.
  - [ ] NumPy reductions.
  - [ ] NumPy broadcasting.
  - [ ] Decide whether pandas is explicitly out of scope for v1.
- [ ] Mapping infrastructure and shared semantics:
  - [ ] Add a typed stdlib mapping registry shared by frontend lowering and Rust codegen.
  - [ ] Represent mapping rules with receiver kind, receiver type, function/member name, argument
        shape, return type, side-effect behavior, and required backend dependencies.
  - [ ] Produce a dedicated unsupported-stdlib diagnostic that names the source API and nearest
        supported alternatives.
  - [ ] Add golden tests for every supported mapping across HIR, MIR, generated Rust, and runtime
        stdout where applicable.
  - [ ] Document each mapping in `specs/stdlib-mapping.md` with source semantics, Rust output, and
        known semantic differences.
  - [ ] Add dependency-injection plumbing for mappings that need crates such as `serde_json`,
        `reqwest`, `chrono`, `regex`, `url`, `ndarray`, or `numpy` bindings.
- [ ] TypeScript array/list mappings:
  - [x] `Array.prototype.includes`.
  - [x] `Array.prototype.map` with capture-free callbacks.
  - [ ] `Array.prototype.map` with captured closures once closure captures land.
  - [x] `Array.prototype.filter` with capture-free callbacks.
  - [x] `Array.prototype.reduce` with explicit initial value and capture-free callback.
  - [ ] `Array.prototype.reduce` without initial value, including empty-array rejection/semantics.
  - [x] `Array.prototype.forEach` with capture-free callbacks.
  - [x] `Array.prototype.find` returning nullable/optional result with capture-free callbacks.
  - [x] `Array.prototype.findIndex` with capture-free callbacks.
  - [x] `Array.prototype.some` with capture-free callbacks.
  - [x] `Array.prototype.every` with capture-free callbacks.
  - [x] `Array.prototype.push`.
  - [x] `Array.prototype.pop`.
  - [x] `Array.prototype.shift`.
  - [x] `Array.prototype.unshift`.
  - [x] `Array.prototype.slice` with positive, omitted, and negative indexes.
  - [ ] `Array.prototype.splice` or explicitly reject with targeted diagnostics.
  - [x] `Array.prototype.concat`.
  - [x] `Array.prototype.join`.
  - [x] `Array.prototype.indexOf` and `lastIndexOf`.
  - [x] `Array.prototype.at`.
  - [x] `Array.prototype.reverse`.
  - [ ] `Array.prototype.sort` with comparator support or explicit rejection.
  - [x] `Array.isArray` as a typed no-op/guard where static types make it decidable.
- [ ] TypeScript string mappings:
  - [x] `String.prototype.toLowerCase`.
  - [x] `String.prototype.toUpperCase`.
  - [x] `String.prototype.includes`.
  - [x] `String.prototype.split(separator)`.
  - [x] `String.prototype.trim`.
  - [x] `String.prototype.trimStart` and `trimEnd`.
  - [x] `String.prototype.startsWith` and `endsWith`.
  - [x] `String.prototype.indexOf` and `lastIndexOf`.
  - [ ] `String.prototype.slice` and `substring`, including Unicode/index semantics decision.
    - [x] `String.prototype.slice` with positive, omitted, and negative indexes.
  - [x] `String.prototype.replace` for literal strings.
  - [ ] `String.prototype.replace` / `replaceAll` with regex once regex support is chosen.
  - [ ] `String.prototype.charAt`, `charCodeAt`, and `at`.
    - [x] `String.prototype.charAt`.
    - [x] `String.prototype.charCodeAt`.
    - [x] `String.prototype.at`.
  - [x] `String.prototype.repeat`.
  - [x] `String.prototype.padStart` and `padEnd`.
  - [ ] `String(...)`, `Number(...)`, and `Boolean(...)` constructors/conversions or explicit
        rejection where JS coercion would be misleading.
- [ ] TypeScript number and `Math` mappings:
  - [x] `Math.abs`.
  - [x] `Math.floor`, `ceil`, `round`, and `trunc`.
  - [x] `Math.max` and `min` for fixed argument lists.
  - [ ] `Math.pow`, `sqrt`, `cbrt`, `hypot`, and exponentiation alignment.
    - [x] `Math.pow`, `Math.sqrt`, `Math.cbrt`, and `Math.hypot`.
  - [x] `Math.sign`.
  - [x] `Math.sin`, `cos`, `tan`, `asin`, `acos`, `atan`, and `atan2`.
    - [x] `Math.sin`, `cos`, `tan`, `asin`, `acos`, and `atan`.
    - [x] `Math.atan2`.
  - [x] `Math.log`, `log10`, `log2`, and `exp`.
  - [x] `Math.random` with an explicit randomness/backend policy.
  - [ ] `Number.isFinite`, `Number.isNaN`, `Number.parseInt`, and `Number.parseFloat`.
    - [x] `Number.isFinite` and `Number.isNaN`.
  - [ ] Numeric formatting: `toString`, `toFixed`, `toPrecision`, and `toExponential`.
- [ ] TypeScript object/record mappings:
  - [x] `Object.keys`.
  - [x] `Object.values`.
  - [x] `Object.entries`.
  - [ ] `Object.fromEntries`.
  - [ ] `Object.assign`.
  - [ ] Object spread/rest once frontend object spread support lands.
  - [x] `hasOwnProperty` / `Object.hasOwn` for record-like values.
  - [ ] `delete obj[key]` or explicit rejection with mutation semantics documented.
- [ ] TypeScript JSON mappings:
  - [x] `JSON.stringify` with `serde_json` dependency injection.
  - [x] `JSON.parse<T>` with typed deserialization strategy for JSON-compatible explicit target types.
  - [ ] Unsupported replacer/reviver/spacing forms produce targeted diagnostics.
  - [ ] Decide and document how classes/interfaces map to serialized shapes.
- [ ] TypeScript Map, Set, Date, RegExp, URL, and Error mappings:
  - [ ] `Map` construction, `get`, `set`, `has`, `delete`, `clear`, `size`, `keys`, `values`,
        `entries`, and iteration.
    - [x] `Map<K, V>` type reference, annotated empty `new Map()`, `new Map([[key, value], ...])`, `Map.has`, and `Map.get`.
    - [x] `Map.set`, `Map.delete`, and `Map.clear`.
    - [x] `Map.size`.
    - [x] `Map.keys`, `Map.values`, and `Map.entries` as list projections.
    - [x] `for...of` over `Map<K, V>` via entry projection.
  - [ ] `Set` construction, `add`, `has`, `delete`, `clear`, `size`, `values`, and iteration.
    - [x] `Set<T>` type reference, `new Set([literal values])`, annotated `new Set()`, and `Set.has`.
    - [x] `Set.add`, `Set.delete`, and `Set.clear`.
    - [x] `Set.size`.
    - [x] `Set.keys`, `Set.values`, and `Set.entries` as list projections.
    - [x] `for...of` over `Set<T>` via value projection.
  - [ ] `Date.now`, construction from timestamp/string, `toISOString`, and basic getters, or
        explicitly defer Date support.
  - [ ] `RegExp.test`, `String.match`, and regex-backed `replace` using the Rust `regex` crate, or
        explicitly defer regex support.
    - [x] `new RegExp(pattern).test(text)` without flags.
  - [ ] `URL` construction and field access through the `url` crate, or explicitly defer URL
        support.
  - [ ] `Error` construction, message access, and throw/catch mapping policy.
- [ ] TypeScript platform and async mappings:
  - [ ] `console.log`, `console.error`, and `console.warn` formatting policy.
  - [ ] `setTimeout`, `clearTimeout`, `setInterval`, and `clearInterval` in async contexts.
  - [x] `fetch` with `reqwest` dependency injection.
  - [ ] `Response.text`, `json`, `status`, `ok`, and headers access.
  - [ ] `Promise.resolve`, `Promise.reject`, `then`, `catch`, and `finally`, or explicit rejection
        in favor of `async`/`await`.
- [ ] Python builtin mappings:
  - [x] `len(...)` for `list`, `dict`, `tuple`, and `str`.
  - [x] `abs(...)` for `int` and `float`.
  - [x] `str(...)`, `int(...)`, `float(...)`, and `bool(...)` conversions with strict semantic
        differences documented.
  - [x] `range(...)` for `for` loops and materialized lists.
  - [x] `enumerate(...)` for list values, dict keys, and set values with default start.
  - [x] `zip(...)` for two list values, dict keys, and set values.
  - [ ] `sum(...)`, `min(...)`, and `max(...)`.
    - [x] `sum(...)` for int and float lists.
    - [x] `min(...)` and `max(...)` for all-int or all-float fixed argument lists.
  - [x] `all(...)` and `any(...)`.
  - [ ] `sorted(...)` and `reversed(...)`.
    - [x] `sorted(...)` for plain sortable lists without `key` or `reverse`.
    - [x] `reversed(...)` for list values.
  - [ ] `list(...)`, `dict(...)`, `tuple(...)`, and `set(...)` constructors.
    - [x] Empty annotated `list()`, `dict()`, `set()`, and `tuple()`.
    - [x] Same-container copy forms: `list(list_value)`, `dict(dict_value)`, `set(set_value)`, and `tuple(tuple_value)`.
    - [x] `list(set_value)` and `list(dict_value)`.
    - [x] `list(tuple_value)`, `set(list_value)`, and `set(tuple_value)` for homogeneous inputs.
    - [x] `dict(list_of_pair_tuples)` for statically typed 2-item tuple pairs.
    - [ ] Remaining cross-iterable constructor forms such as `tuple(list_value)` and arbitrary `dict(iterable_pairs)`.
  - [ ] `isinstance(...)` and `issubclass(...)` where static types make them decidable.
  - [ ] `print(...)` formatting parity and stderr support if needed.
- [ ] Python string mappings:
  - [x] `str.lower()`.
  - [x] `str.upper()`.
  - [x] `str.strip()` with no arguments.
  - [x] `str.split(separator)`.
  - [x] String `in` / `not in`.
  - [x] `str.lstrip()` and `str.rstrip()`.
  - [x] `str.startswith()` and `str.endswith()`.
  - [ ] `str.find()`, `index()`, `rfind()`, and `rindex()`.
    - [x] `str.find()` and `str.rfind()`.
    - [ ] `str.index()` and `str.rindex()` remain unsupported because missing-value exception semantics are not modeled yet.
  - [x] `str.replace()`.
  - [x] `str.join()`.
  - [x] `str.removeprefix()` and `removesuffix()`.
  - [ ] `str.isdigit()`, `isalpha()`, `isalnum()`, and related predicates.
    - [x] `str.isdigit()`, `str.isalpha()`, and `str.isalnum()`.
  - [ ] f-string lowering beyond the currently supported literal/string-concat subset, if needed.
- [ ] Python list/tuple/set/dict mappings:
  - [x] List `in` / `not in`.
  - [x] Set `in` / `not in`.
  - [x] Tuple `in` / `not in`.
  - [x] Dict key `in` / `not in`.
  - [ ] `list.append`, `extend`, `insert`, `pop`, `remove`, `clear`, `copy`, `count`, `index`,
        `reverse`, and `sort`.
    - [x] `list.append`.
    - [x] `list.extend`.
    - [x] `list.insert`.
    - [x] `list.pop`.
    - [x] `list.remove`.
    - [x] `list.clear`.
    - [x] `list.copy`.
    - [x] `list.count`.
    - [x] `list.index`.
    - [x] `list.reverse`.
    - [x] `list.sort`.
  - [x] Tuple indexing/slicing parity with Python negative indexes for static integer indexes and static slice bounds.
  - [ ] Dict `get`, `setdefault`, `keys`, `values`, `items`, `update`, `pop`, `clear`, and `copy`.
    - [x] Dict `get`.
    - [x] Dict `setdefault`.
    - [x] Dict `keys`, `values`, and `items`.
    - [x] Dict `update`.
    - [x] Dict `pop`.
    - [x] Dict `clear`.
    - [x] Dict `copy`.
  - [ ] Set construction and `add`, `remove`, `discard`, `contains`, `union`, `intersection`,
        `difference`, `symmetric_difference`, `isdisjoint`, `issubset`, `issuperset`, and iteration.
    - [x] Set literals and `in` / `not in`.
    - [x] Set `add`, `remove`, `discard`, `clear`, and `copy`.
    - [x] Set `union`, `intersection`, `difference`, and `symmetric_difference`.
    - [x] Set `isdisjoint`, `issubset`, and `issuperset`.
    - [x] Set iteration in `for` loops via value projection.
  - [x] Dict iteration in `for` loops via key projection.
  - [ ] List, dict, and set comprehensions once comprehension lowering is in scope.
- [ ] Python standard-library module mappings:
  - [ ] `math`: `floor`, `ceil`, `trunc`, `sqrt`, `pow`, `sin`, `cos`, `tan`, logs, `isfinite`,
        `isnan`, constants `pi`, `e`, and `tau`.
    - [x] `math.floor`, `ceil`, and `trunc`.
    - [x] `math.sqrt` and `pow`.
    - [x] `math.sin`, `cos`, `tan`, `asin`, `acos`, and `atan`.
    - [x] `math.atan2`.
    - [x] `math.log`, `log10`, `log2`, and `exp`.
    - [x] `math.isfinite` and `isnan`.
  - [ ] `json`: `loads`, `dumps`, `load`, and `dump` with `serde_json`.
    - [x] `json.dumps`.
    - [x] `json.loads` for annotated JSON-compatible destination types.
  - [ ] `re`: `compile`, `search`, `match`, `fullmatch`, `sub`, and `split`, or explicitly defer
        regex support.
    - [x] `re.search`, `re.match`, and `re.fullmatch` with direct string pattern/text arguments.
  - [ ] `datetime`: `datetime`, `date`, `timedelta`, `now`, `utcnow`, parsing, and formatting, or
        explicitly defer datetime support.
  - [ ] `pathlib` / `os.path`: path join, basename/name/stem/suffix, exists, is_file, is_dir.
  - [ ] `os`: environment reads/writes, cwd, mkdir, makedirs, remove, and rename.
  - [ ] `sys`: argv, stdin/stdout/stderr basics.
  - [ ] `random`: random, randint, choice, shuffle with an explicit RNG policy.
    - [x] `random.random`.
  - [ ] `collections`: `defaultdict`, `Counter`, and `deque`, or targeted rejection.
  - [ ] `itertools`: `chain`, `islice`, `count`, `repeat`, `product`, and `zip_longest`, or
        targeted rejection.
  - [ ] `functools`: `partial`, `reduce`, `lru_cache`, or targeted rejection.
  - [ ] `typing`: runtime no-op handling for `cast`, `assert_never`, `TypeGuard`, and aliases.
- [ ] Python IO and networking mappings:
  - [ ] `open(...)` read/write text mode.
  - [ ] `open(...)` binary mode.
  - [ ] Context-manager lowering for files.
  - [x] `requests` or `urllib` support decision; prefer explicit crate-backed mappings over a
        broad compatibility shim.
  - [ ] Async HTTP mapping compatible with the Phase 5 runtime model.
- [ ] Python native/data-library mappings:
  - [ ] NumPy array construction and dtype model.
  - [ ] NumPy shape, ndim, size, indexing, slicing, reshape, transpose, and astype.
  - [ ] NumPy elementwise arithmetic, comparisons, reductions, and broadcasting policy.
  - [ ] NumPy `zeros`, `ones`, `empty`, `arange`, `linspace`, `concatenate`, `stack`, and `where`.
  - [ ] Decide per NumPy operation whether it lowers to Rust-native arrays/crates, C ABI calls, or
        a deliberately hybrid backend.
  - [ ] Pandas support decision: explicit defer unless a concrete mapping plan exists.
- [ ] Phase 6 exit criteria:
  - [ ] Every checked mapping has unit tests in the relevant frontend crate.
  - [ ] Every checked mapping has MIR lowering coverage.
  - [ ] Every checked mapping has Rust codegen coverage.
  - [ ] End-to-end fixtures cover at least 30 stdlib cases across TypeScript and Python.
  - [ ] Unsupported stdlib calls produce source-located diagnostics rather than generic call errors.
  - [ ] `cargo test`, `cargo check`, and `cargo clippy` pass after each completed stdlib slice.

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
