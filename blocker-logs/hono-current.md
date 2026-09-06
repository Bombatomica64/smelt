<!-- Regenerate with:
       smelt --manifest-path third_party/hono/Smelt.toml probe --format md
     Paths below are relative to the Hono checkout. -->

# Probe report: hono

- Transpile: **no** — whole-crate build aborts at `src/http-exception.ts`
- Files scanned: 286 · with blockers: 10

## Blockers by category

| Category | Occurrences |
| --- | ---: |
| missing-stdlib | 6 |
| unresolved-reference | 6 |
| unsupported-lowering | 6 |

## Distinct blocker classes

| Occurrences | Files | Category | Blocker class | Example |
| ---: | ---: | --- | --- | --- |
| 6 | 4 | missing stdlib | unresolved class `X` | `src/client/client.ts` |
| 6 | 3 | unresolved reference | unresolved identifier `X` | `src/client/client.ts` |
| 2 | 1 | non-working Rust (unlowered) | JSON.stringify() value must be JSON-serializable (…) | `src/request.ts` |
| 1 | 1 | non-working Rust (unlowered) | module-level mutable binding `X` is written through (…); only whole-value reassignment of a non-primitive mutable global is lowered | `src/router/reg-exp-router/router.ts` |
| 1 | 1 | non-working Rust (unlowered) | rest parameter type must resolve to an array type | `src/client/types.ts` |
| 1 | 1 | non-working Rust (unlowered) | string replace requires string-compatible receiver, pattern, and replacement | `src/client/utils.ts` |
| 1 | 1 | non-working Rust (unlowered) | string search methods require string receiver and argument | `src/utils/url.ts` |

<details>
<summary>Full messages for 2 elided blocker class(es)</summary>

- **JSON.stringify() value must be JSON-serializable (…)**
  - Example: `src/request.ts`
  - Message:
    ```text
    JSON.stringify() value must be JSON-serializable (got Some(Class { name: Symbol(446), args: [] }), class `BodyInit`)
    ```
- **module-level mutable binding `X` is written through (…); only whole-value reassignment of a non-primitive mutable global is lowered**
  - Example: `src/router/reg-exp-router/router.ts`
  - Message:
    ```text
    module-level mutable binding `wildcardRegExpCache` is written through (`wildcardRegExpCache[key] = …` or `wildcardRegExpCache.field = …`); only whole-value reassignment of a non-primitive mutable global is lowered
    ```
</details>

