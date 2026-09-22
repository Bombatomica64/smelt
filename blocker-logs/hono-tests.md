# Hono generated tests — phase-3 baseline

Clone `honojs/hono` @ `eebdf7be39abf0a872671835ccce0c4f03ea497a` + `.github/compat/hono/.`
(overlay with the 64 `phase 3 pending` test exclusions), fresh full-feature `smelt`,
`smelt build --manifest-path <abs>` from the repo root: 63 modules, **211 `#[test]`** in 10 test
modules (21 in-scope test files; the others are type-only or empty after lowering).

## `cargo check --tests` (3 min 38 s, limit 30 min) — 402 errors, test binary does not compile

| code | n | what it is |
| --- | ---: | --- |
| E0308 | 285 | 238 `expected SmeltUnknown, found String` (238 of them in `router/trie-router/node.test.ts`), 28 `expected SmeltList<Rc<dyn Fn..>>, found String`, 13/4 union-arm mismatches, 2 `SmeltBody` vs `String` |
| E0609 | 69 | `name` / `match_` read as FIELDS on `RegExpRouter<SmeltUnknown>` / `PatternRouter<SmeltUnknown>` (the shared `router/common.case.test.ts` harness takes the router as an interface value) |
| E0615 | 26 | `add` / `match_` taken as a value on the same router types (method read without a call) |
| E0560 | 22 | struct literal fields `router`/`routes`/`use_`/… on `Hono` |

By test module: `trie-router/node` 238, `reg-exp-router/router` 79, `vercel/handler` 45,
`timeout` 20, `powered-by` 12, `pattern-router/router` 6, `utils` 2.

`cargo test --no-fail-fast`: **did not compile** (same errors). Phase-3 baseline:
**0 passed / 0 failed / 211 total (not compiled)**.

## SmeltUnknown (phase 4 advisory baseline)

`blocker-logs/smelt-unknown-baseline-hono.json`, same 63-file crate: 101488 occurrences —
runtime-prelude 6265, legitimate-boundary 86945, **avoidable-erasure 8278**. Advisory only (never
blocks; the CI hono job diffs against it). The top five avoidable shapes: **610** ×
`let _smelt_tmp_N: SmeltUnknown;` (17 files: temporaries declared before their typed
initializer is known); **306** × the string-index fallback
`smelt_key.parse::<usize>().ok().and_then(|index| value.chars().nth(index)…)` (4 files: a
computed key on an erased receiver whose static type is a string or record); **279** ×
`SmeltUnknown::String(value) => {` (7 files: runtime narrowing arms of values that were erased
upstream); **227** × `_ => SmeltUnknown::Undefined,` (6 files: the default arm of those same
erased dispatches); **192** × `let _smelt_tmp_N: SmeltRecord<String, SmeltUnknown> =
SmeltRecord::from([...])` (2 files: record literals whose values are all concrete but share one
erased value type).
