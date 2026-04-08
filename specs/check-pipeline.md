# Check Pipeline

This document defines the ordered phases that run during `smelt check` and at the start of `smelt build`. Both commands run the full check pipeline — build continues into HIR lowering and codegen only if all phases pass.

## Phases (TypeScript)

```
Source files
     │
     ▼
1. oxclint          — lint: style, forbidden patterns, no-any enforcement
     │
     ▼
2. tsgo --noEmit     — type-check: strict mode, catches type errors
     │
     ▼
3. smelt rules      — smelt-specific rejections: constructs tsgo accepts but smelt cannot lower
     │
     ▼
4. HIR construction — parse and walk the AST into smelt-hir nodes
```

## Phases (Python)

```
Source files
     │
     ▼
1. ty               — type-check + lint in one pass
     │
     ▼
2. smelt rules      — smelt-specific rejections
     │
     ▼
3. HIR construction
```

## Rationale for the Order

**tsgo over tsc** — smelt targets `tsgo` (the native Go port of the TypeScript compiler, TypeScript 7+) rather than the original `tsc`. tsgo is in alpha while smelt is in alpha; both should reach stability around the same time. tsgo is significantly faster than tsc and will be the canonical TypeScript compiler going forward. If tsgo is not available in the environment, smelt errors out — it does not fall back to tsc.

**oxclint before tsgo** — oxclint is faster and catches forbidden patterns (dynamic access, `any`, unsafe constructs) before spending time on full type inference. Fail fast on the cheap checks first.

**tsgo before smelt rules** — smelt rules walk a type-annotated AST. Running tsgo first means smelt rules can assume every node has a resolved type; no need to handle untyped nodes defensively.

**smelt rules before HIR** — smelt rules are the gate between "valid TypeScript" and "TypeScript smelt can lower". They reject constructs that tsgo accepts but have no HIR representation (e.g. conditional types, index signatures, decorators). Producing HIR for a rejected construct would be silently wrong.

## smelt Rules

smelt rules are a separate validation pass, not part of the HIR construction walk. They operate on the raw AST (oxc `Program`) before any HIR node is created.

Current rejections (v1.0):

| Construct | Reason |
|---|---|
| `any`, `unknown`, `never` | No HIR type representation |
| Conditional types (`T extends U ? X : Y`) | Not lowerable |
| Mapped types (`{ [K in T]: U }`) | Not lowerable |
| Index signatures (`[key: string]: T`) | No MIR equivalent |
| `eval`, `Function()`, `with` | Fundamentally dynamic |
| Decorators | Deferred to v1.1 |
| `var` | `let`/`const` only |
| JSX/TSX | Out of scope |
| Namespaces | Out of scope |

Each rejection produces a `smelt_frontend_ts::Error` with a source span and a message explaining why the construct is unsupported, not just that it is.

## Error Handling

Phases run sequentially. If a phase produces errors, the pipeline stops and reports them — later phases are not run because they may depend on invariants the failed phase was supposed to establish (e.g. running smelt rules on an un-typechecked AST is meaningless).

All errors use the format:

```
path/to/file.ts:12:5: error[smelt::no-conditional-types]: conditional types cannot be lowered to HIR
```

The `error[code]` format makes errors grep-able and linkable to documentation.

## Cross-Language Note

For v1.0, each pipeline runs independently — TS files through the TS pipeline, Python files through the Python pipeline. Cross-language import resolution happens later in HIR lowering (a v1.x milestone). The check pipeline has no cross-language phase yet.
