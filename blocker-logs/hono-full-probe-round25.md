# Probe report: hono

- Transpile: **no** — whole-crate build aborts at `/home/user/smelt/.claude/worktrees/agent-a1dc6a2aff708b0a8/third_party/hono/src/utils/crypto.ts`
- Files scanned: 258 · with blockers: 1

## Blockers by category

| Category | Occurrences |
| --- | ---: |
| unsupported-lowering | 1 |

## Distinct blocker classes

| Occurrences | Files | Category | Blocker class | Example |
| ---: | ---: | --- | --- | --- |
| 1 | 1 | non-working Rust (unlowered) | `X` hashes a concrete byte view (…); the erased typed-array views are not modeled as digest input yet | `/home/user/smelt/.claude/worktrees/agent-a1dc6a2aff708b0a8/third_party/hono/src/utils/crypto.ts` |

<details>
<summary>Full messages for 1 elided blocker class(es)</summary>

- **`X` hashes a concrete byte view (…); the erased typed-array views are not modeled as digest input yet**
  - Example: `/home/user/smelt/.claude/worktrees/agent-a1dc6a2aff708b0a8/third_party/hono/src/utils/crypto.ts`
  - Message:
    ```text
    `crypto.subtle.digest` hashes a concrete byte view (the value `TextEncoder.encode` and the body readers answer); the erased typed-array views are not modeled as digest input yet
    ```
</details>

