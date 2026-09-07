# Probe report: hono

- Transpile: **no** — whole-crate build aborts at `/home/user/smelt/.claude/worktrees/agent-ae773f0327d1831f5/third_party/hono/src/context.ts`
- Files scanned: 258 · with blockers: 2

## Blockers by category

| Category | Occurrences |
| --- | ---: |
| unsupported-lowering | 2 |

## Distinct blocker classes

| Occurrences | Files | Category | Blocker class | Example |
| ---: | ---: | --- | --- | --- |
| 1 | 1 | non-working Rust (unlowered) | Response init is an erased value, so its keys cannot be read with their types | `/home/user/smelt/.claude/worktrees/agent-ae773f0327d1831f5/third_party/hono/src/hono-base.ts` |
| 1 | 1 | non-working Rust (unlowered) | field access is only lowered for Record<string, T>, class, and interface values for now (…) | `/home/user/smelt/.claude/worktrees/agent-ae773f0327d1831f5/third_party/hono/src/context.ts` |

<details>
<summary>Full messages for 1 elided blocker class(es)</summary>

- **field access is only lowered for Record<string, T>, class, and interface values for now (…)**
  - Example: `/home/user/smelt/.claude/worktrees/agent-ae773f0327d1831f5/third_party/hono/src/context.ts`
  - Message:
    ```text
    field access is only lowered for Record<string, T>, class, and interface values for now (receiver: Float, field: status)
    ```
</details>

