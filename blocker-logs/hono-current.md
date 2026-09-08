# Probe report: hono

- Transpile: **no** — whole-crate build aborts at `/home/user/smelt/third_party/hono/src/utils/crypto.ts`
- Files scanned: 258 · with blockers: 7

## Blockers by category

| Category | Occurrences |
| --- | ---: |
| unsupported-lowering | 9 |

## Distinct blocker classes

| Occurrences | Files | Category | Blocker class | Example |
| ---: | ---: | --- | --- | --- |
| 2 | 2 | non-working Rust (unlowered) | unknown class method `X` | `/home/user/smelt/third_party/hono/src/router/trie-router/node.ts` |
| 2 | 1 | non-working Rust (unlowered) | assignment operator is not lowered yet: BitwiseOR | `/home/user/smelt/third_party/hono/src/utils/buffer.ts` |
| 1 | 1 | non-working Rust (unlowered) | JSON.stringify() value must be JSON-serializable (…) | `/home/user/smelt/third_party/hono/src/utils/crypto.ts` |
| 1 | 1 | non-working Rust (unlowered) | array callback methods currently require arrow function callbacks | `/home/user/smelt/third_party/hono/src/utils/html.ts` |
| 1 | 1 | non-working Rust (unlowered) | new Set(…) currently requires an array argument | `/home/user/smelt/third_party/hono/src/router/reg-exp-router/node.ts` |
| 1 | 1 | non-working Rust (unlowered) | string prefix/suffix methods require string receiver and argument | `/home/user/smelt/third_party/hono/src/utils/body.ts` |
| 1 | 1 | non-working Rust (unlowered) | switch continue lowering is not implemented yet | `/home/user/smelt/third_party/hono/src/utils/html.ts` |

<details>
<summary>Full messages for 2 elided blocker class(es)</summary>

- **JSON.stringify() value must be JSON-serializable (…)**
  - Example: `/home/user/smelt/third_party/hono/src/utils/crypto.ts`
  - Message:
    ```text
    JSON.stringify() value must be JSON-serializable (got Some(Union([TypeId(29), TypeId(196), TypeId(197), TypeId(430), TypeId(432)])))
    ```
- **new Set(…) currently requires an array argument**
  - Example: `/home/user/smelt/third_party/hono/src/router/reg-exp-router/node.ts`
  - Message:
    ```text
    new Set(iterable) currently requires an array argument
    ```
</details>

