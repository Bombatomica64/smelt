# Erasure reasons

Design adapted from Bun's `--aot` (oven-sh/bun#45631), `src/sema/aot_rules.rs`
and `docs/bundler/aot-adoption.mdx`.

When Smelt cannot carry a value's static type into Rust it gives the value
`Type::Unknown`, emitted as the tagged runtime value `SmeltUnknown`. Every place
that makes that decision now names **why**, at the decision, and records it
with the source span it was lowering:

- TypeScript frontend: `ModuleBuilder::erase(reason)` / `erase_at(reason, span, detail)`
  (`crates/smelt-frontend-ts/src/lowering/erasure.rs`). The span is the
  innermost type annotation, expression or statement being lowered (the
  *anchor*), or a finer span the site passes explicitly. A site that only
  compares against `unknown`, or rebuilds a type whose erasure was already
  decided upstream, uses `unknown_type()`, which records nothing.
- Python frontend: `erase(reason, span, detail)` in `lowering/type_helpers.rs`.
- Rust backend: `rust_type` records the four erasures only codegen decides
  (`crates/smelt-codegen-rust/src/emitter/backend_erasure.rs`). Their span is
  the owning function's body (the entry block), because `rust_type` renders a
  `TypeId`, not a local; the detail names the type parameter, class or union.

Records live in `smelt_hir::Crate::erasures` (frontends) and are returned by
`smelt_codegen_rust::emit_crate_with_modules_reporting` (backend).

## Reasons and classes

| Reason | Class | Decided where | Meaning |
| --- | --- | --- | --- |
| `source-unknown` | legitimate | TS annotations | the source says `unknown` |
| `explicit-any` | avoidable | TS annotations, `as any` | the author opted out with `any` |
| `unfilled-generic` | avoidable | TS generics | a type argument is unfilled or defaults to `any` |
| `type-level` | avoidable | TS annotations | conditional/indexed/mapped types, intersections, mixed-arity function unions Smelt does not evaluate |
| `unmodeled-type` | avoidable | TS | `symbol`, callable-object surfaces |
| `open-layout` | legitimate | TS annotations | `object`, `{}`, index signatures with open values (Bun's open-layout) |
| `missing-annotation` | avoidable | TS/Python | no annotation and failed inference (`[]`, `new Map()`, unannotated params/returns, `*args`) |
| `untyped-builtin` | avoidable | TS stdlib | a builtin whose TS result type is precise but Smelt does not model yet |
| `external-type` | legitimate | TS stdlib | `JSON.parse`, `Response.json()`: data from outside the type system (Bun's external-type) |
| `untyped-package` | legitimate | TS imports | a value from a module Smelt has no types for (Bun's untyped-package) |
| `by-name` | avoidable | TS references | globals, `this`, `require`, members of unknown layout (Bun's by-name) |
| `dynamic-operator` | avoidable | TS operators | an operator that widens non-erased operands (mixed literals, `?:`, `||`, spread) |
| `runtime-narrowing` | legitimate | TS guards | the value is inspected through runtime narrowing (Bun's can-throw) |
| `test-harness` | legitimate | TS testing | `vi.fn()`, matchers: compared dynamically by the test runtime |
| `dynamic-metadata` | legitimate | Python/TS specialization | dynamic metadata the source language makes dynamic |
| `erased-generic` | avoidable | codegen | a type parameter no Rust generic carries here |
| `erased-union` | avoidable | codegen | a union that is not a concrete generated union |
| `unresolved-class` | avoidable | codegen | a class type naming no lowered class |
| `never-value` | avoidable | codegen | `never` rendered as the tagged value |
| `unclassified` | avoidable | anywhere | a site with no reason yet (default `warn`) |

The class follows the `SmeltUnknown boundaries` policy: legitimate when the
**source** type is itself dynamic, avoidable when the source has a static shape
Smelt failed to carry. Unknown names in a stale sidecar count as avoidable.
`smelt_hir::erasure` tests pin the table (`reason_classes_follow_policy`).

## Severities

`allow` (record only), `warn` (print `warning[erasure/<reason>]: file:line:col: message`),
`deny` (print `error[erasure/<reason>]` and fail). Defaults follow Bun's table:
everything `allow`, except `unclassified = "warn"`, which marks a Smelt gap.

```toml
[erasure]
default = "allow"
explicit-any = "warn"

[[erasure.scope]]
paths = ["src/core/**"]
missing-annotation = "deny"
```

CLI: `smelt build|check --erasure <reason>[:<glob>]=<level>` (repeatable, `all`
for every reason). Settings apply in order — reason defaults, manifest
`default`, manifest reasons, manifest scopes, CLI flags — and the last match
wins. `check` sees frontend reasons only (the backend runs in `build`).
`check --message-format json` adds `category: "erasure"` diagnostics with
`code: "erasure/<reason>"` and a `severity` field (`warning`/`error`); lowering
diagnostics omit `severity` as before.

## Sidecar and report

`smelt build` writes `<output>/smelt-erasure.json`: every record resolved to
`file` (manifest-relative), `line`, `column`, `reason`, `class`, `origin`
(`frontend`/`backend`), `detail`, `severity`, sorted and deduplicated, plus
per-reason totals; it is rewritten only when its bytes change.
`smelt smelt-unknown-report` reads a sidecar beside (or inside) a scanned
directory, or one passed with `--erasure <path>`, and adds an "Avoidable erasure
by reason and source line" section (and an `erasure` object in JSON). The
existing per-generated-line classification is unchanged; the section explains
it, it does not replace it.

## Limits

- A record means the decision was made; a speculative lowering whose result is
  discarded can still leave its record (over-reporting, never hiding).
- Backend records are attributed to the function, not the local.
- `check` cannot report backend reasons.
