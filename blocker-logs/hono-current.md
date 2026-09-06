# Probe report: hono

- Transpile: **no** — whole-crate build aborts at `/home/user/smelt/third_party/hono/src/request.ts`
- Files scanned: 258 · with blockers: 3

## Blockers by category

| Category | Occurrences |
| --- | ---: |
| unsupported-lowering | 5 |

## Distinct blocker classes

| Occurrences | Files | Category | Blocker class | Example |
| ---: | ---: | --- | --- | --- |
| 2 | 1 | non-working Rust (unlowered) | JSON.stringify() value must be JSON-serializable (…) | `/home/user/smelt/third_party/hono/src/request.ts` |
| 1 | 1 | non-working Rust (unlowered) | Request init is an erased value, so its keys cannot be read with their types | `/home/user/smelt/third_party/hono/src/hono-base.ts` |
| 1 | 1 | non-working Rust (unlowered) | Response init type declares none of its modeled keys | `/home/user/smelt/third_party/hono/src/context.ts` |
| 1 | 1 | non-working Rust (unlowered) | field access is only lowered for Record<string, T>, class, and interface values for now (…) | `/home/user/smelt/third_party/hono/src/context.ts` |

<details>
<summary>Full messages for 2 elided blocker class(es)</summary>

- **JSON.stringify() value must be JSON-serializable (…)**
  - Example: `/home/user/smelt/third_party/hono/src/request.ts`
  - Message:
    ```text
    JSON.stringify() value must be JSON-serializable (got Some(Class { name: Symbol(364), args: [] }), class `BodyInit`)
    ```
- **field access is only lowered for Record<string, T>, class, and interface values for now (…)**
  - Example: `/home/user/smelt/third_party/hono/src/context.ts`
  - Message:
    ```text
    field access is only lowered for Record<string, T>, class, and interface values for now (receiver: Float, field: status)
    ```
</details>

