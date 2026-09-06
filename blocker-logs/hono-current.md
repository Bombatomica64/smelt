# Probe report: hono

- Transpile: **no** — whole-crate build aborts at `/home/user/smelt/third_party/hono/src/request.ts`
- Files scanned: 258 · with blockers: 3

## Blockers by category

| Category | Occurrences |
| --- | ---: |
| unsupported-lowering | 4 |

## Distinct blocker classes

| Occurrences | Files | Category | Blocker class | Example |
| ---: | ---: | --- | --- | --- |
| 1 | 1 | non-working Rust (unlowered) | Request init is an erased value, so its keys cannot be read with their types | `/home/user/smelt/third_party/hono/src/hono-base.ts` |
| 1 | 1 | non-working Rust (unlowered) | Request init type declares none of its modeled keys | `/home/user/smelt/third_party/hono/src/request.ts` |
| 1 | 1 | non-working Rust (unlowered) | dynamic computed method names are not lowered yet | `/home/user/smelt/third_party/hono/src/request.ts` |
| 1 | 1 | non-working Rust (unlowered) | field access is only lowered for Record<string, T>, class, and interface values for now (…) | `/home/user/smelt/third_party/hono/src/context.ts` |

<details>
<summary>Full messages for 1 elided blocker class(es)</summary>

- **field access is only lowered for Record<string, T>, class, and interface values for now (…)**
  - Example: `/home/user/smelt/third_party/hono/src/context.ts`
  - Message:
    ```text
    field access is only lowered for Record<string, T>, class, and interface values for now (receiver: Float, field: status)
    ```
</details>

