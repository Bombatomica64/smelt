# TypeScript 7 type parity

How much of what the TypeScript 7 checker knows about a program survives
Smelt's TypeScript lowering. Measured with the embedded TS 7.1.0-beta checker
(tsrs `89923ab5`, `crates/smelt-ts-types`):

```sh
smelt ts7-parity-report <Smelt.toml | file.ts>... [--format json] [--output report.md]
```

For every HIR expression the report looks up the checker's type for the node
at the same source span and classifies the pair (rules in
`crates/smelt-transpiler/src/ts7_parity.rs`):

- **agree**: Smelt's HIR type has the shape TypeScript resolved.
- **smelt-erased**: Smelt lowered to `Unknown` a value TypeScript types
  concretely. This is the main gap.
- **wider**: Smelt keeps `Optional<T>` or a union around the `T` TypeScript
  proved, usually a flow narrowing Smelt did not apply (or `noUncheckedIndexedAccess`-style
  honesty on index reads).
- **mismatch**: both types are concrete but their shapes differ. This covers
  representation choices and real bugs alike.
- **ts-dynamic** / **ts-deferred**: TypeScript's own `any`/`unknown` or a
  still-generic operator. These are never counted against Smelt.

**Parity = agree / (agree + smelt-erased + wider + mismatch).**

## Baseline (2026-10-26, branch `feat/tsrs-type-oracle`)

| corpus | parity | agree | smelt-erased | wider | mismatch | ts-dynamic |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| examples (150 end-to-end) | 96.5% | 10,492 | 76 | 143 | 159 | 212 |
| hono `0f5ad7b` | 91.8% | 15,281 | 554 | 256 | 564 | 1,443 |
| remeda `3c80f28` | 91.0% | 21,313 | 932 | 78 | 1,090 | 4,509 |
| es-toolkit `e008a28` | 86.6% | 24,555 | 1,227 | 160 | 2,404 | 5,232 |
| ts-pattern `c92ca43` | refused | — | — | — | — | — |

Full per-corpus reports: `ts7-parity-{examples,hono,remeda,es-toolkit,ts-pattern}.md`.

Caveats:

- The checker only sees the TypeScript sources the crate lowered. `vitest`
  (TS2307) and hono's global `describe`/`expect` (TS2304/TS2593) are
  unresolved, so test-file expressions that touch them are `ts-dynamic`. They
  sit outside the denominator, so parity is not inflated, but test files are
  under-measured. Shipping the vitest declarations Smelt already models
  would close this.
- ts-pattern does not lower at all (`too many generic type arguments`,
  `src/patterns.ts:150`). It is the type-level stress corpus and the clearest
  single symptom of the gap.
- A shape comparison does not prove behaviour. It shows whether the type
  TypeScript proved reached HIR. Generic arguments of nominal types are not
  compared yet, and a class matches by original source name.

## Largest gap families (worklist)

1. **Uninstantiated generics at call sites** (mismatch). es-toolkit has
   `List<TypeParam(T)> ← number[]` 290× and `← string[]` 51×. HIR keeps the
   callee's `T` on the call result where TS7 instantiated it. Inference
   results belong in HIR (the hand-port bar: `chunk::<f64>`).
2. **Erased element types** (mismatch `List<Unknown> ← T[]`). es-toolkit has
   83 + 73 + 58; remeda has 139 + 23 + 20. Array literals and spreads whose
   element type TS7 knows exactly.
3. **Primitives erased after narrowing** (smelt-erased `Unknown ← number`/`string`).
   es-toolkit has 106 + 74, remeda 61 + 43, hono 66 + 25. For example
   `value.size` after an `instanceof Map` guard (`examples/105`), and `e.name`
   in a `catch` narrowed by `instanceof UnsupportedPathError` (`examples/129`).
4. **Callable objects with properties** (smelt-erased `Function & {…}`).
   remeda has 188 (`lazyDataLastImpl`, erasure `unmodeled-type`).
5. **Generic class references erased under type-level arguments**.
   hono has `Unknown ← Hono<…>` 109× (erasure `type-level`) and `Context<…>`
   54×. TS7 resolves the arguments; Smelt erases the whole value.
6. **Representation choices to revisit against the hand-port bar**
   (mismatch). `URL`, `RegExp` and `Error` are lowered as `String` (hono has
   62 + 40 + 26, es-toolkit 53 for `Error`). A Rust team would use
   `url::Url`, `regex::Regex` and an error struct.
7. **Unapplied flow narrowing** (wider `Optional<T> ← T`). This family
   appears in all corpora.

## Roadmap

- **Phase 0, done**: `smelt-ts-types` embeds the tsrs TS7 checker and
  returns span-keyed, Smelt-owned type facts plus TS diagnostics.
- **Phase 1a, done**: `smelt ts7-parity-report` provides the measurement
  above.
- **Phase 1b**: ratchet. Check in the JSON baselines and gate CI on "no
  family grows", the same way `smelt-unknown-report` does.
- **Phase 2**: make the checker the source of truth in the frontend, family
  by family, in the worklist order above. `FrontendOptions` carries the facts,
  lowering asks `type_at(span)` before it re-derives a type, and the
  hand-rolled inference it replaces gets deleted. Each family must hold its
  behaviour and type-preservation requirements (`CLAUDE.md`).
- **Phase 3**: replace `npx tsc`/`oxlint` in `checker.rs` with the in-process
  TS7 diagnostics, so "valid TypeScript" means TS7-valid.
- **Phase 4**: language coverage. Run a runtime-equivalence corpus over the
  strict-typed subset of the TypeScript conformance suite and test262 module
  tests (Bun `--aot` passes 31,231 test262 files).
- **Phase 5**: a big-program target, transpiling a large real program end to
  end (Bun's demo is the 370k-line TypeScript 6 compiler).

## Packaging note

tsrs crates are `publish = false`, so `smelt-ts-types` is a git dependency.
crates.io rejects git dependencies even when they are optional, so
`smelt-transpiler`'s default `ts7` feature blocks `scripts/publish-crates.sh`
until tsrs publishes, Smelt vendors it, or the publish script strips the
feature.
