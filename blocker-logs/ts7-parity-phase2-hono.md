# TS7 parity, phase 2, round 1: lowering consumes checker facts (Hono)

Branch `feat/tsrs-type-oracle` (PR #285). Measured on Hono v5 `0f5ad7b6` (the
CI pin), with the `.github/compat/hono` overlay, building with `smelt build`
and then `cargo test --no-fail-fast -- --test-threads=4` on `dist-smelt`.
The build was capped at `MemoryMax=12G`, ran with `CARGO_BUILD_JOBS=4`, and
had debuginfo off.

| run | generated suite |
| --- | --- |
| `origin/main` binary (baseline) | **357 passed / 97 failed** of 454 |
| this round | **395 passed / 59 failed** of 454 |

No test that passed before fails now. Every test that newly passes is a
middleware app test (powered-by, cors, pretty-json, timeout, trailing-slash
and mount request flows).

## Rules added

Each rule is general: it is stated in terms of TypeScript semantics, never a
library spelling.

1. **Checker-evaluated annotations** (`lowering/checker_types.rs`,
   `checker_evaluated_or` in `ty/annotations.rs`). When an annotation's own
   lowering erases to `Unknown`, the frontend asks the TypeScript 7 checker
   what the type node evaluates to (`getTypeFromTypeNode`).
   - The evaluated type is re-spelled as TypeScript type syntax that resolves
     in the module and lowered through `ts_type_to_hir`. All lowering
     conventions therefore stay in one place.
   - Structure is spelled first. An alias name is used only as a fallback,
     never for the alias's own body. Names are spelled only if they are
     visible here.
   - The rule declines for: dynamic or deferred top-level types, free type
     parameters, the polymorphic `this`, structural object shapes, and the
     apparent-type roots (`Object`, `Function`).
   - Behaviour is that of hand-writing the evaluated type. Type preservation:
     the value keeps the type TypeScript proved instead of `Unknown`.
   - Example `172_checker_evaluated_annotation_types` covers a conditional
     type, indexed access and `ReturnType<C['m']>`, compared against Node.
     `65_typeof_keyof_alias_union`'s `Record<'Content-Type', Mime>` arm
     becomes `Dict<String, String>`.

2. **Checker-typed callback parameters** (`arrow_callback_param_types_with_hint`).
   An unannotated identifier parameter whose contextual type Smelt lost or
   erased takes the checker's type.
   - A class or interface declared in another crate module resolves by
     declaration (declaring module plus original name), at its default type
     arguments. This is what a bare `(c: Context)` annotation lowers to.
   - Hono's handlers `(c) => c.text('x')` now read `c.text` through the typed
     getter. Before, the getter was read through an erased field and gave
     `undefined`, so the response was a 404.

3. **Trailing list spread into typed fixed parameters** (`spread_arguments.rs`).
   `f(...xs)` into a callee with fixed, concrete parameters and no rest
   lowers to `xs[0]`, `xs[1]`, and so on, which is JavaScript's binding.
   Erased callees keep the runtime packed-spread path.

4. **Lowering order follows value imports only** (`order_manifest_sources`).
   Type-only imports are erased and do not order ES module evaluation.
   - Following them let a type-only cycle (`context.ts -> types.ts ->
     hono-base.ts`) lower `hono-base.ts` before `compose.ts`, whose value it
     imports. The `compose` call then bound an erased, empty import, so
     dispatch never ran its handlers.
   - This is the root cause of the `null`-printing 500s in the middleware
     tests.

5. **Hoisted record-to-class conversions** (codegen
   `EmitContext::record_conversion_helpers`). An outermost
   erased-record-to-class conversion is emitted once per type as
   `__smelt_from_unknown_record_<n>` and called from every site.
   - The helper body is the text that used to be inlined, so behaviour is
     unchanged by construction.
   - Hono's generated sources shrink from 57 MB to 12 MB. The 28.5 MB CORS
     test module shrinks to 2.1 MB.
   - The test crate now compiles at a 3.6 GiB peak. Before, rustc was
     OOM-killed at the 12 GiB cap.
   - The SmeltUnknown report keys its boundary classification on the helper
     prefix (`record_conversion_helper_is_a_boundary`, mutation-checked), so
     the relocated adapter keeps the classification it had inline.

## Ratchets (this round's final tree)

| baseline | before | after |
| --- | ---: | ---: |
| examples SmeltUnknown avoidable (invariant) | 0 | 0 |
| es-toolkit SmeltUnknown avoidable (ratchet) | 22,451 | 22,372 |
| hono SmeltUnknown avoidable (advisory) | 19,578 | 16,937 |
| remeda SmeltUnknown avoidable (advisory) | 24,167 | 22,490 |
| typedness, examples and es-toolkit (ratchets) | n/a | no program less typed; re-snapshotted with `--update-baseline` |

The typedness ratchet caught two rule defects before the commit:

- Distributing a spread into an erased callee (es-toolkit `isEqualWith`)
  added erased reads. Distribution is now limited to typed parameters.
- An alias body was re-spelled as its own name (`partial`'s `Placeholder`).
  Self-reference is now declined.

## Remaining Hono failures (59)

| file | failures |
| --- | ---: |
| trailing-slash | 18 |
| mount | 13 |
| utils/cookie | 11 |
| pretty-json | 7 |
| cors | 3 |
| timeout | 3 |
| compose | 1 |

The cookie failures are all signed cookies, which need HMAC through
`crypto.subtle`.

Also found and not yet fixed:

- An arrow `(...args) => ..` assigned to a fixed-arity function type keeps
  its rest signature, so the generated Rust does not compile. TypeScript
  types `args` as the tuple of the contextual parameters; the fix is to bind
  the rest parameter as that tuple.
- Awaiting an async closure read back from an array produced no output.

## Not verified locally

- The es-toolkit and remeda generated-crate tests (CI runs them).
- Radash and the other compat probes.
