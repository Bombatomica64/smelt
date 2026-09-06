# Probe report: hono

- Transpile: **no** — whole-crate build aborts at `/home/user/smelt/third_party/hono/src/http-exception.ts`
- Files scanned: 258 · with blockers: 6

## Blockers by category

| Category | Occurrences |
| --- | ---: |
| missing-stdlib | 2 |
| unsupported-lowering | 6 |

## Distinct blocker classes

| Occurrences | Files | Category | Blocker class | Example |
| ---: | ---: | --- | --- | --- |
| 2 | 2 | missing stdlib | unresolved class `X` | `/home/user/smelt/third_party/hono/src/context.ts` |
| 2 | 1 | non-working Rust (unlowered) | JSON.stringify() value must be JSON-serializable (…) | `/home/user/smelt/third_party/hono/src/request.ts` |
| 1 | 1 | non-working Rust (unlowered) | field access is only lowered for Record<string, T>, class, and interface values for now (…) | `/home/user/smelt/third_party/hono/src/context.ts` |
| 1 | 1 | non-working Rust (unlowered) | module-level function return type needs a supported default value | `/home/user/smelt/third_party/hono/src/hono-base.ts` |
| 1 | 1 | non-working Rust (unlowered) | module-level mutable binding `X` is written through (…); only whole-value reassignment of a non-primitive mutable global is lowered | `/home/user/smelt/third_party/hono/src/router/reg-exp-router/router.ts` |
| 1 | 1 | non-working Rust (unlowered) | string search methods require string receiver and argument | `/home/user/smelt/third_party/hono/src/utils/url.ts` |

<details>
<summary>Full messages for 3 elided blocker class(es)</summary>

- **JSON.stringify() value must be JSON-serializable (…)**
  - Example: `/home/user/smelt/third_party/hono/src/request.ts`
  - Message:
    ```text
    JSON.stringify() value must be JSON-serializable (got Some(Class { name: Symbol(363), args: [] }), class `BodyInit`)
    ```
- **field access is only lowered for Record<string, T>, class, and interface values for now (…)**
  - Example: `/home/user/smelt/third_party/hono/src/context.ts`
  - Message:
    ```text
    field access is only lowered for Record<string, T>, class, and interface values for now (receiver: Float, field: status)
    ```
- **module-level mutable binding `X` is written through (…); only whole-value reassignment of a non-primitive mutable global is lowered**
  - Example: `/home/user/smelt/third_party/hono/src/router/reg-exp-router/router.ts`
  - Message:
    ```text
    module-level mutable binding `wildcardRegExpCache` is written through (`wildcardRegExpCache[key] = …` or `wildcardRegExpCache.field = …`); only whole-value reassignment of a non-primitive mutable global is lowered
    ```
</details>

