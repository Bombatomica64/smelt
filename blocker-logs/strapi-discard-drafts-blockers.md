# Strapi `5.0.0-discard-drafts.ts` Frontend Blockers

Probe:

```sh
cargo run -q --bin smelt -- build --manifest-path /tmp/Smelt.strapi-core.toml
```

Original grouped blocker file:

`third_party/strapi/packages/core/core/src/migrations/database/5.0.0-discard-drafts.ts`

Current status:

- `5.0.0-discard-drafts.ts` now lowers through the Strapi probe.
- `migrations/draft-publish.ts` also lowers after handling `for await` over promise-like batches, erased index receivers, and Strapi `async.map(..., { concurrency })`.
- The current first blocker has moved into `services/session-manager.ts`. Fixed there so far: Buffer-style `crypto.randomBytes(16).toString('hex')` and reading class methods as function-valued fields for `.bind(...)` assignments.

## Blocker Groups

- `new Map() requires a Map<K, V> type annotation`
  - Representative source: `recordClonedComponentPair`, where a nested cache lazily creates `pairMap = new Map()`.
  - Practical lowering: infer `Map<unknown, unknown>` for unannotated Map construction when no contextual type is available.

- `Array.from currently supports object sources shaped as { length }`
  - Representative source: `Array.from(idMap.keys())`, `Array.from(cloneMap.entries())`, and similar iterable Map/Set surfaces.
  - Practical lowering: accept unknown, Map iterator, Set iterator, and collection-like sources as opaque lists.

- `local trx is not callable (Union(...))`
  - Representative source: Knex transaction object used as a callable query builder, e.g. `trx(tableName)`.
  - Practical lowering: when a local has a union containing unknown/class-like pieces, allow opaque call lowering instead of rejecting.

- `nullish coalescing fallback must match the non-nullish value type`
  - Representative source: schema/database metadata fallback expressions where one side is erased and the other is concrete.
  - Practical lowering: for erased or union operands, coerce the fallback into the chosen result type instead of rejecting.

- `Map.set key and value must match the map type`
  - Representative source: migration caches initialized or flowed through erased data and then populated with concrete IDs.
  - Practical lowering: for unknown-key/value Map surfaces, coerce inserted key/value or widen to erased map behavior.

- `array callback parameter count is not supported for this method`
  - Representative source: callback forms using fewer/more callback parameters than the current array helper path expects.
  - Practical lowering: ignore extra JS callback parameters and synthesize missing supported parameters where possible.

- `callback side-effect blocks only support expression statements`
  - Representative source: callback blocks with local control flow before a returned value.
  - Practical lowering: extend callback block lowering to tolerate common statement forms or fall back to closure-body lowering.

- `method calls are only lowered for class values for now`
  - Representative source: fluent/opaque query builder method calls on erased Knex-like values.
  - Practical lowering: permit opaque method calls on unknown/class-like unions.

- `tuple element type is not lowered yet: TSAnyKeyword`
  - Representative source: tuple annotations containing `any`.
  - Practical lowering: map `any` tuple elements to `Unknown`.

- `Number requires a primitive argument`
  - Representative source: `Number(...)` over erased metadata/query values.
  - Practical lowering: allow unknown/class-like values by casting/coercing to numeric output.

- `Math.min requires number arguments`
  - Representative source: `Math.min(...)` over values with erased or union numeric surfaces.
  - Practical lowering: coerce unknown/class-like numeric candidates to `Float`.

- `number.toString radix argument must be numeric`
  - Representative source: `crypto.randomBytes(16).toString('hex')` in `services/session-manager.ts`.
  - Practical lowering: when `.toString(...)` is called on an erased/non-number receiver with a string argument, treat the argument as an encoding-like option and lower the result as a string conversion instead of numeric radix formatting.

- `unknown class or interface field generate_session_id on SessionManager`
  - Representative source: `api.generateSessionId = sessionManager.generateSessionId.bind(sessionManager)`.
  - Practical lowering: resolve class methods as function-valued members when they are read through property access instead of directly called.

- `unresolved class ValidationError`
  - Representative source: `const { ValidationError } = errors; throw new ValidationError(...)` in `services/webhook-store.ts`.
  - Practical lowering: allow module-global values obtained from opaque imported objects to be used as opaque constructors.

- `destructured abstract method parameters are not lowered yet`
  - Representative source: `async executeListener({ event, info }: Event)` in `services/webhook-runner.ts`.
  - Practical lowering: method-signature precollection can synthesize parameter names for destructured parameters while keeping the declared parameter type.

- `unresolved identifier AbortSignal`
  - Representative source: `AbortSignal.timeout(10000)` in `services/webhook-runner.ts`.
  - Practical lowering: treat common Node/Web platform globals as opaque module globals when no local declaration exists.

- `array forEach statement receiver must be an array`
  - Representative source: `this.webhooksMap.forEach((webhooks, event) => ...)` in `services/webhook-runner.ts`.
  - Practical lowering: statement-form `forEach` should accept Map/Dict receivers by projecting values, matching expression-form callback opacity.

- `RegExp.test() requires exactly one string argument`
  - File: `services/entity-validator/blocks-validator.ts`.
  - Count: 5 occurrences in the current Strapi probe.
  - Likely lowering: accept erased or structurally narrowed values by coercing the tested value to string when TypeScript already allowed the source.
  - Fixed by routing only real RegExp-producing `.test(...)` receivers through regex lowering, so Yup validation `.test(...)` calls are left to opaque method lowering.

Current next blocker after these fixes:

- `array concat requires an array receiver`
  - File: `services/entity-validator/validators.ts`.
  - Current first blocker after `RegExp.test()` fixes.

## Priority

Start with the highest-repeat and lowest-risk opacity shims:

1. Untyped `new Map()`.
2. Opaque callable unions for Knex-style `trx(...)`.
3. `Array.from(...)` over iterable/unknown collection surfaces.
4. Nullish coalescing fallback coercion.
5. Map mutation coercion.
