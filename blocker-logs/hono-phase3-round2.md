# Hono phase 3, round 2 — the test binary compiles

Clone `honojs/hono` @ `eebdf7be39abf0a872671835ccce0c4f03ea497a` + `.github/compat/hono/.` (overlay
unchanged: the 64 `phase 3 pending` exclusions stay), fresh full-feature `smelt`, `smelt build
--manifest-path <abs>` from the repo root.

| gate | round 1 | round 2 |
| --- | --- | --- |
| `smelt build` | 63 modules, 211 `#[test]` | 63 modules (280 tests in the binary) |
| `cargo check` (no tests) | 0 errors | 0 errors |
| `cargo check --tests` | **402 errors** | **0 errors** |
| `cargo test --no-fail-fast` | did not compile | compiles; `router/trie-router/node.test.ts` hangs (see H8 below) |
| `cargo test -- --skip __smelt_module_node_test` | — | **71 passed / 121 failed / 88 not run** (the hanging module) |

Every fix is a general lowering/codegen rule with a fixture diffed against Node:
`examples/typescript/end-to-end/121_callback_into_union_function_arm` (golden HIR/MIR/Rust), and
five runtime fixtures under `crates/smelt-transpiler/tests/fixtures/runtime/` run by
`build_runs_runtime_fixtures` (built and RUN, not golden-diffed: their surrounding programs cross
pre-existing erasures — a source-`unknown` instantiation, a `Promise<any>` body reader, a
destructured-parameter record — that would break the examples corpus's avoidable == 0
invariant for reasons unrelated to the rule under test).

## Fixed families (402 → 0)

| # | errors | family | rule | fixture |
| ---: | ---: | --- | --- | --- |
| R1 | 238 | `expected SmeltUnknown, found String` at `node.insert(.., 'get root')` on `new Node()` (`T` inferred `unknown`) | A method argument whose declared type is a bare class type parameter is coerced to the type the RECEIVER pins (`Node<Arg>` ⇒ `Arg`); a receiver argument the caller cannot spell (the class's own uninstantiated `T`) is the source `unknown` instantiation and crosses `erase`. `emitter/call.rs::receiver_bound_class_param_argument_text`. | `receiver_pinned_class_parameter` |
| R2 | 91 | `name`/`match_`/`add` read as fields of `RegExpRouter<_>` (E0609/E0615), `Hono { .. }` literal on a tuple newtype (E0560), `Hono_1` field reads | Class → interface (and class → class) structural conversion: an interface's callable slot is filled by the source class's METHOD bound to the instance (`is_interface_method_slot_bound_by_source`); a reference-class source is read through `.0.borrow()`; a reference-class TARGET is built as `T(Rc::new(RefCell::new(TInner { .. })))`; the bound method's parameters/return are instantiated at the source's class arguments (`source_class_substituted_ty`). `emitter/core.rs`. | `class_value_at_interface_type` |
| R3 | 28 | `app.use('/p', h)` against `MiddlewareHandlerInterface` packed the path into the handler list (E0308) | A callable-interface FIELD call selects its overload from the argument count and probed argument types (`callable_static_member_call` now uses `function_member_type_for_args`), and a rest overload checks each absorbed argument against the rest ELEMENT type (`signature_accepts_arg_types`). | `overloaded_callable_interface_field` |
| R4 | 13 | `expected SmeltUnion1206, found SmeltUnknown` at `router.match(..)` | A class/interface slot whose DECLARED return is a union over the class's type parameters has no generated enum and renders `SmeltUnknown`; the call's source type is then `Unknown` and the destination extracts (`declared_slot_return_is_erased`, both the terminator and `Rvalue::ClosureCall` paths). | `generic_slot_union_return` |
| R5 | 4 | `timeout(1100, () => new HTTPException(..))` into `HTTPExceptionFunction \| HTTPException` (E0308) | A function source injected into a concrete union's unique function arm (fewer-parameter callbacks adapt through the ordinary function coercion); the erased dispatch of a union-typed callee erases the union enum first. | e2e `121_callback_into_union_function_arm` |
| R6 | 2 | `expect(content).toBe('This is index')` with `content: ReadableStream \| null` (E0308 `Option<SmeltBody> == Option<String>`) | An equality matcher whose operands are unrelated scalars/instances (neither assignable to the other) compares the erased runtime values, as the JS matcher does. Collections and already-erased operands keep their existing structural path. | (frontend) |
| R7 | 1 | `res.json` read as a field of `SmeltResponse` | `Response.json()` / `Request.json()` modeled as body readers: `text()` + the fallible `JSON.parse` adapter (catchable `SyntaxError`), `Promise<any>` result (a genuine dynamic boundary: JSON text has no static shape). | `response_request_json` |

Found while writing the fixtures, also fixed:

* **Callable-interface slot from a function** (the runtime blocker behind every Hono app test):
  `this.use = (arg1, ...handlers) => ..` stored `Default::default()` into the
  `MiddlewareHandlerInterface` record, so every `app.get`/`app.use` called an inert default.
  A function stored into a callable-interface record now fills `__smelt_call`; an overloaded
  interface's erased variadic slot is filled through the source's own erased-callable adapter
  (positional runtime arguments → typed parameters, trailing rest gathered)
  (`coercion.rs::callable_interface_from_function_text`). Fixture `overloaded_callable_interface_field`.
* **`serde_json` `preserve_order`**: `JSON.stringify(JSON.parse(s))` sorted keys (`serde_json::Map`
  was a `BTreeMap`). The generated manifest now enables `preserve_order`. Fixture `response_request_json`.

## SmeltUnknown

* examples invariant: avoidable **0 → 0** (`--fail-on-regression` passes).
* es-toolkit ratchet: avoidable **30547 → 30547**. A first cut of R6 erased mismatched COLLECTION
  and mixed-width numeric `toEqual`/`toBe` operands (and `Awaited<unknown>` instances) into extra
  temporaries (+25); R6 is now restricted to scalars/instances with fully known type arguments,
  and `Int` vs `Float` operands keep their numeric comparison.
* hono (advisory, never blocks): avoidable 8278 → **8821** (+543). By shape: +238
  `node.insert(.., SmeltUnknown::String(..))` is R1 — `new Node()` is `Node<unknown>` in `tsc`,
  a source-`unknown` instantiation the textual classifier cannot tell from erasure; +280/+243
  `Rc<dyn Fn(&SmeltUnknown, &SmeltUnknown) -> SmeltUnknown>` defaults are R3's ambiguous-overload
  call type for callable-interface fields that previously claimed the first overload (net of
  −162/−84 for the old, wrong first-overload spellings); +74 `Router { .. }` is R2's member-wise
  adapter over the erased `Router<[unknown, RouterRoute]>` instantiation. None is a new internal
  ABI erasure; the test crate simply compiles through paths it could not reach before.
* Gates re-run on this head: radash `384 passed; 3 failed` (pin holds), remeda
  `1787 passed; 2 failed` (pin holds), es-toolkit `cargo check` + `cargo test --no-run` clean.

## Open, found this round (next queue)

| # | family | evidence |
| ---: | --- | --- |
| H8 | `router/trie-router/node.test.ts`: every test in the module loops (RSS grows past 2 GB; the unfiltered run is SIGKILLed) | `test_all_all_methods`, `test_basic_usage_*` "running for over 60 seconds". Trie `search` loop miscompile; reproduce with the module alone. |
| H9 | A lifted top-level arrow whose parameter DEFAULT reads a module-level const (`exception = defaultTimeoutException`, Hono `middleware/timeout`) gets `{}`/default instead of the const | Reproduced while writing fixture `121_callback_into_union_function_arm` (default inlined there). |
| H10 | `unknown class method set` / `callback method X is not lowered into closure bodies` — both are a callable-interface FIELD called on a callback PARAMETER (`(c, next) => c.set(..)`); the top-level field call path now works, the closure-body path does not | Blocks the 7 overlay files under that comment (still excluded). |
| H11 | `toStrictEqual` failures: 42 in `utils/encode.test.ts` | `expect(got).toStrictEqual(want)` — byte-array comparison. |
| H12 | `router/common.case.test.ts` via RegExpRouter: 22 failures, panics in `matcher.rs` | runtime, after R2 made the harness compile. |
| H13 | `body.test.ts` 24 failures (`parseBody` form/urlencoded, `rejects.toThrow`) | runtime. |
| H14 | `typeof x === 'function'` does not narrow a concrete `Fn \| Class` union, so the called result goes through the erased dispatch (`ex` in fixture `121_callback_into_union_function_arm` is `SmeltUnknown`) | avoidable erasure, pre-existing. |

Remaining round-1 brief rows (host streams, dynamic `import()`, `LabeledStatement`, `Context`
cross-kind collision, …) are unchanged; see `hono-phase3-round2-brief.md`.
