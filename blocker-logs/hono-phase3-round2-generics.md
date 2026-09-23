# Hono phase 3, round 2 — Agent S: type-parameter defaults, callable-interface fields, the `Context` collision

Brief rows 2 and 3 of the head of the queue plus the "Cross-kind `Context` collision" row of
`hono-phase3-round2-brief.md`. Branch `worktree-agent-aa7111d6443760ca8`, merged with
`origin/lorenzo/great-rubin-vy0943` @ `80c6545c` (Agents R and T) before the final gates.

Every Hono number below: fresh clone of `honojs/hono` @ `eebdf7be…` + `.github/compat/hono/.`,
fresh full-feature `smelt`, absolute `--manifest-path`, `cargo check --message-format=short`.

## What the E0107 actually was

Row 2 was written as "fill omitted type arguments from the defaults". That rule already existed
(`type_arguments_with_defaults`, `class_reference_takes_trailing_type_parameter_defaults`), and a
fixture with three defaulted parameters referenced with 0, 1 and 3 arguments
(`examples/typescript/end-to-end/122_defaulted_type_arguments`) passes on the base. Every E0107 on
the Hono build was the SAME symbol problem as the collision row: a `Context<..>` reference that
resolved to `router/reg-exp-router/node.ts`'s zero-parameter `interface Context` instead of
`context.ts`'s class (rendered `Context_1`). Three paths spelled the class by its bare name:

| path | where | fix |
| --- | --- | --- |
| type alias predeclared before the module's imports were recorded | `types.ts`'s `ErrorHandler<E> = (err, c: Context<E>) => ..` (cycle with `context.ts`); every alias user inherited it (`compose.rs` `Context<SmeltUnknown>`) | `ModuleBuilder::record_import_provenance` runs in `predeclare_type_declarations_with_path` before the alias pass |
| import through a barrel | `import { Context } from '../..'` in the eight `conninfo` adapters, `helper/route` | `propagate_type_renames_through_reexports` (transpiler) + `scan_type_reexports` (frontend): a barrel inherits the rename of every ambiguous name it re-exports (`export *`, `export { A as B } from`, `import {A}; export {A}`), to a fixpoint |
| `new Context(..)` in a module that lowers first | `hono-base.ts` `#dispatch` built an erased external object typed as the interface | `new_expression_with_hint` resolves an imported ambiguous name through `imported_ambiguous_type_name`, and constructs by that symbol (defaults filling the type arguments) when the class is not lowered yet |

Multi-module regression: `build_resolves_an_ambiguous_class_through_cycles_barrels_and_new`
(`hir_cli_cross_language_tests.rs`) — interface and class of one name, alias cycle, barrel, `new`,
built and run.

## Callable-interface fields (row 3)

`unknown class method set` was not a missing call path: `Context.set: Set<E>` names the module's
own `interface Set<E> { <Key ..>(key, value): void; .. }`, and the builtin reference table mapped
`Set<..>` to the JS `Set` collection regardless. Rules landed:

* **Library-global shadowing** (`type_reference_to_hir`, `LIB_GLOBAL_TYPE_NAMES`): a module that
  declares an interface/class/enum named like a library global skips the builtin table. A
  same-named TYPE ALIAS keeps the table — es-toolkit's `type Capitalize<T>` restates the utility,
  and lowering its conditional template body erased 39 sites (measured, then excluded).
  `source_declares_type` now matches on identifier boundaries (`SetOptions` is not `Set`).
* **Function → callable-interface coercion** (`emitter/coercion.rs`,
  `function_into_callable_object_slot_ty` + `callable_object_with_slot_text`): assigning an arrow
  to a field typed by a callable interface rendered `Default::default()`, so every later
  `obj.set(..)` silently called the interface's no-op default. It now stores the (adapted) function
  in `__smelt_call`. Fixture `123_callable_interface_field` prints `3`/`4` like Node.
* **Overload selection at a callable field** (`callable_static_member_call`): the call's arity and
  probed argument types select the signature (was: the first declared), and when several match
  but exactly one is fully concrete, that one is the ABI (`interface_call_signature_type_for_args`).
* **Callback bodies**: `callback_callable_field_method_to_body_expr` accepts a callable-interface
  field (not only `Type::Function`), and an erased receiver takes the same field-read + bound
  `ClosureCall` the non-callback path uses (`callback_erased_receiver_method_call`) — this is what
  `compose.test.ts`'s `c.req.header(..)` inside a middleware needed.

## Code size: shared record-class extraction helpers

Correctly typing `Context` made every erased middleware adapter inline a field-by-field
`SmeltUnknown -> Context_1` rebuild (~230 KB per site): `middleware/timeout`'s test module went
0.8 → 8 MB and `cargo check --tests` did not finish in 30 min. `shared_record_extractor_call`
emits that extraction ONCE per non-generic target as `fn __smelt_from_record_<Type>` at the crate
root (flushed like the function-item accessors), with the body the inline arm renders at an empty
record-conversion stack, so semantics are unchanged. After: `dist-smelt/src` 11 MB total, `main.rs`
6.6 → 6.1 MB, `cargo check --tests` 94 s. The helper signature is classified a legitimate boundary
(`fn __smelt_from_record_`, `shared_record_extractor_is_a_boundary`).

## Also

* `validate_implements`: an instance getter satisfies an implemented interface property
  (Agent T's note). Test `an_instance_getter_satisfies_an_implemented_interface_property`.

## Overlay (`.github/compat/hono/Smelt.toml`)

Re-admitted (9): `compose.test.ts`; `adapter/{bun,cloudflare-pages,cloudflare-workers,deno,netlify,vercel}/conninfo.test.ts`;
`adapter/bun/server.test.ts`; `helper/route/index.test.ts`. Moved to the family that now blocks them:

| file | next blocker |
| --- | --- |
| `types.test.ts`, `helper/factory/index.test.ts` | import `hc` from the excluded `client` |
| `hono.test.ts` | dynamic `import()` in `utils/color.ts` |
| `middleware/request-id/index.test.ts` | E0428: a named function expression shadowing its `const` is also emitted as an empty module-level `fn request_id()` |
| `middleware/language/index.test.ts` | lowers; reaches `utils/accept.ts` + `utils/cookie.ts` (library family) + 11 E0308 `SmeltUnion` vs `SmeltUnknown` |
| `middleware/secure-headers/index.test.ts` | lowers; E0428 (same shadowing), E0381, E0369 `==` on `Option<ContentSecurityPolicyOptions>` |
| `adapter/aws-lambda/conninfo.test.ts` | 8 E0277: `ClientCert` derives `Deserialize` over a `SmeltRecord` field |
| `adapter/cloudflare-workers/serve-static.test.ts` | pulls `middleware/serve-static/index.ts`, a 37 MB module (duplication class) |

## Numbers

| gate | result |
| --- | --- |
| `smelt build` | 82 modules, **234 `#[test]`** in 19 test modules (was 63 / 211) |
| `cargo check` | **0 errors** |
| `cargo build` | links |
| `cargo check --tests` | 94 s, **421 errors**: E0308 259, E0560 154, E0282 4, E0609 2, E0605 1, E0369 1 (baseline 308: E0308 285, E0560 22, E0609 1) |

Of the 421, 141 are in the newly admitted modules: `helper/route/index.test.ts` 138 (E0560 132 +
E0282 4 + E0609 1 + E0369 1 — a derived reference class passed where its base is expected is
rebuilt as a `Hono_1 { .. }` struct literal) and `compose.test.ts` 3. The pre-existing modules went
308 → 280 (E0308 285 → 257, E0560 22 → 22, E0609 1 → 1).
