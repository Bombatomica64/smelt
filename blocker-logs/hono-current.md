<!-- Regenerate with:
       smelt --manifest-path third_party/hono/Smelt.toml probe --format md
     Paths below are relative to the Hono checkout. -->

# Probe report: hono

- Transpile: **no** — whole-crate build aborts at `src/http-exception.ts`
- Files scanned: 258 · with blockers: 5

## Blockers by category

| Category | Occurrences |
| --- | ---: |
| missing-stdlib | 3 |
| unsupported-lowering | 4 |

## Distinct blocker classes

| Occurrences | Files | Category | Blocker class | Example |
| ---: | ---: | --- | --- | --- |
| 3 | 3 | missing stdlib | unresolved class `X` | `src/context.ts` |
| 2 | 1 | non-working Rust (unlowered) | JSON.stringify() value must be JSON-serializable (…) | `src/request.ts` |
| 1 | 1 | non-working Rust (unlowered) | field access is only lowered for Record<string, T>, class, and interface values for now (…) | `src/context.ts` |
| 1 | 1 | non-working Rust (unlowered) | string search methods require string receiver and argument | `src/utils/url.ts` |

<details>
<summary>Full messages for 2 elided blocker class(es)</summary>

- **JSON.stringify() value must be JSON-serializable (…)**
  - Example: `src/request.ts`
  - Message:
    ```text
    JSON.stringify() value must be JSON-serializable (got Some(Class { name: Symbol(363), args: [] }), class `BodyInit`)
    ```
- **field access is only lowered for Record<string, T>, class, and interface values for now (…)**
  - Example: `src/context.ts`
  - Message:
    ```text
    field access is only lowered for Record<string, T>, class, and interface values for now (receiver: Float, field: status)
    ```
</details>

