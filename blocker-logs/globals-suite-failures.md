# Vitest-globals suites: failure families after Agent N's globals change

Agent N made test files that use Vitest/Jest globals without importing them into
real test modules (`blocker-logs/hono-phase3-round1.md`). That turned previously
silent radash suites into real ones and exposed the families below. Every count
here is from a clean clone, a freshly built full-feature `smelt`, and
`smelt rust-test-report --full` (`blocker-logs/radash-globals-failures.md`,
`blocker-logs/remeda-globals-failures.md`).

| Corpus | Before (base `ba6641e8`) | After (this branch) |
| --- | --- | --- |
| radash (`4cab1900` + compat + brief `sed`) | 358 passed / 29 failed (387) | **384 passed / 3 failed** (387) |
| remeda (`3c80f28b` + compat) | 1786 passed / 3 failed (1789) | **1787 passed / 2 failed** (1789) |

Correction to the round-1 note: remeda's three failures at base were TWO bigint
tests plus `sample.test.ts` "returns a random result", not three bigint tests.
The third one is the `Math.random`-as-a-value family below (now fixed).

## Fixed families (general rules, no library spelling)

| # | Family | Tests | Rule | Where | Pinned by |
| --- | --- | --- | --- | --- | --- |
| 1 | `Array.from(gen)` / `[...gen]` over a sync generator erased the generator and panicked "unknown is not iterable" | radash array `list` x3, async `parallel`, series `spin` x3 | drain a `Generator<Y,..>` into `List<Y>` through a typed cast + prelude `SmeltGenerator::collect_yields` | `frontend expr/operators.rs`, `codegen emitter/list.rs`, `lib.rs` prelude | e2e `116_generator_array_from` |
| 2 | `ns.f(a, b)` via `import * as ns` did not pack a rest callee's arguments | radash curry `compose` x3 | namespace member calls pack trailing args into the rest `ListLit` exactly like named calls | `frontend stmt/assignments.rs` | `build_runs_namespace_call_to_overloaded_rest_function` |
| 3 | compact callback IR dropped object spreads (`({ ...a, ...b })` built `{}`) | radash curry `partob` x2 | refuse the spread and fall back to closure-body lowering; an erased-merge no longer adopts a later `Dict<String,f64>`; a differently-typed record source is value-converted instead of skipped | `frontend callbacks/dispatch.rs`, `expr/operators.rs`, `codegen emitter/map.rs` | runtime `callback_destructuring_runtime::an_object_spread_in_a_concise_callback_keeps_every_source`, unit `lowers_callback_object_literal_with_opaque_spread` |
| 4 | optional-chained read of an absent key on an erased-value record panicked "missing field", then decoded `undefined` as `Some(union)` | radash async `retry` x8 (first half) | an erased record reads an absent key as `undefined`; the optional narrowing covers wide unions carried as `SmeltUnknown` | `codegen emitter/call_runtime.rs`, `optional_access.rs` | runtime `optional_record_field_runtime::an_absent_optional_field_of_a_wide_union_record_reads_undefined` |
| 5 | an exported arrow const referenced before its declaration bound a default-returning placeholder | radash async `retry` x8 (second half: `retry` -> `tryit`) | exported forward-referenced arrows join the dependency-ordered arrow queue | `frontend module_init.rs` | e2e `117_forward_exported_arrow` |
| 6 | a concise callback calling a variadic function item (`console.log`) dropped its arguments | (found while reproducing 5) | a called fixed-arity function item is called directly, not through a closure of its declared arity | `frontend callbacks/closures.rs` | e2e `118_concise_callback_call_args` |
| 7 | `jest.fn()` read an unmodeled `jest` global, so mocks never counted | radash curry `debounce` x4 | `jest` is the Jest spelling of the Vitest `vi` mock namespace | `frontend lowering/stdlib.rs`, `decls/types_iface.rs` | runtime `erased_call_dispatch_runtime::a_jest_global_mock_is_the_vitest_mock`, unit `lowers_the_jest_global_mock_namespace_like_vi` |
| 8 | arrow-const parameter defaults were dropped (omitted arg arrived as `0`) | radash array `cluster` (capacity overflow) | arrow params get the function-declaration `Optional<T>` ABI + `apply_parameter_defaults` prelude | `frontend decls/arrows.rs` | e2e `119_arrow_param_default` |
| 9 | an array method calling an erased variadic callback bound the element to the rest slot | radash curry `chain` mapper over `User[]` | pass `(item, index, array)` as the rest list; read a non-erased declared return back out of `SmeltErasedFunction::call` | `codegen emitter/list_query.rs` | runtime `erased_call_dispatch_runtime::a_variadic_array_callback_receives_every_array_method_argument` |
| 10 | `Math.random` read as a value was `undefined` (every call `NaN`) | remeda `sample` "returns a random result" | model `Math.random/abs/floor/ceil/round/trunc/sign/sqrt/max/min` as builtin member values | `stdlib builtin_members.rs`, `codegen builtin_member_prelude.rs` | runtime `global_namespace_value_runtime::a_math_function_read_as_a_value_computes` |
| 11 | `Math.sqrt(2)` emitted `2.0.sqrt()` (ambiguous float) | (found by the family-10 tier) | unary math operands are cast `as f64` | `codegen emitter/numeric.rs` | same tier |

## Not fixed (and why)

### A. `as any` type-violating inputs (radash, 2 tests) — test-semantics, not a Smelt gap

- `array.test.ts` "boil does not fail when provided array is funky shaped":
  `_.boil({} as any, () => true)` passes an OBJECT to `array: readonly T[]`.
- `number.test.ts` "inRange handles nullish values":
  `_.inRange(0, 1, null as any)` passes `null` to `end?: number`.

Both tests deliberately lie to the type checker with `as any` to probe runtime
guards (`array.length ?? 0`, `typeof end === 'undefined'`). The declared
parameter types (`T[]`, `number | undefined`) cannot represent an object or a
`null`: the list conversion rejects the object ("unknown is not iterable"), and
`null` and `undefined` both land in `None`, so `typeof end` answers
`'undefined'` where Node answers `'object'`. Making them pass would require
widening these parameters to `SmeltUnknown`, which the SmeltUnknown policy
forbids ("never to make things compile or pass"). Proposed rule: none in the
emitter; if the campaign wants these green, it is a harness policy call — a
compat-overlay note that `as any` inputs outside the parameter's declared domain
are out of scope for typed lowering.

### B. `Proxy` traps (radash, 1 test) — unmodeled runtime feature

`curry.test.ts` "callable makes object callable": `_.callable(obj, fn)` returns
`new Proxy(Object.assign(FUNC, obj), { get, set, apply })`. `new Proxy` lowers
to its TARGET and drops the handler (`new_expr.rs::proxy_constructor_expression`),
so calling the proxy runs the no-op `FUNC` and `call('23').doors` is
`undefined`. Proposed rule: model `Proxy` as a runtime object kind carrying
`target` + `handler`, with `smelt_get_unknown_field` / the erased field write /
the `__smelt_call` dispatch consulting `handler.get` / `handler.set` /
`handler.apply` before the target. A genuine dynamic boundary (handler traps are
arbitrary code on arbitrary keys). Not attempted here: it touches every erased
read/write/call path in the prelude.

### C. bigint literal wider than i64 (remeda, 2 tests) — needs a bigint type

`randomBigInt.test.ts` "tiny ranges with huge numbers" (plain and crypto
polyfill) uses `9_999_999_999_999_999_999_999n`. Today TS `bigint` lowers to
`Type::Int` (`i64`) and a literal that does not fit falls back to `f64`, so
`randomBigInts(HUGE, HUGE + 1n)` runs through `i64` parameters and the
comparison against `1e22` is lossy. A per-literal carrier cannot fix this: the
value flows into `randomBigInt(from: bigint, to: bigint)`, whose parameters are
`i64`. `Type::Int` is also the index/integer type (`closure_arg_1: i64`), so it
cannot simply be re-rendered.

Proposed rule (design, not landed):
1. HIR `Type::BigInt` + `Literal::BigInt(String)`; `TSBigIntKeyword`, bigint
   literal types and `BigInt(x)` lower to it (today `BigInt(x)` is `ToFloat`).
2. Emit `num_bigint::BigInt` (dependency added only when the type is used);
   literals via `BigInt::from(i64)` when they fit, `parse` otherwise; `+ - * / %`
   on references, `<< >>` with a `usize` shift, `& | ^`, `pow`, comparisons,
   `to_str_radix`, `console.log` printing `123n`, `typeof` → `"bigint"`.
3. A `SmeltUnknown::BigInt` erased variant (every exhaustive prelude match
   gains an arm) so matchers and unions do not round-trip through `f64`.
Scope: every `Type` match across HIR/MIR/codegen plus the prelude's
`SmeltUnknown` matches, and it moves remeda's `add/subtract/multiply/sum/...`
bigint paths off `i64`, so it needs its own campaign with the remeda gate as the
guard. Flagged for a decision rather than rejected.

## Other defects found and left alone (not in these corpora)

- A named object type alias (`type RetryOptions = {...}`) with a callback
  member lowers to an erased class whose optional field read asks
  `type_id(Type::Unknown)` in a crate without it: "type table does not contain
  literal operand type Unknown at emitter/call_runtime.rs" (the H14 shape). The
  inline object type (radash's spelling) is fixed above.
- `Array.from(source, mapper)` with a non-object source evaluates and DROPS the
  mapper (`operators.rs::array_from_call`).
- `vi.clearAllMocks()` / `jest.clearAllMocks()` are unmodeled and fall to an
  erased no-op call; harmless in radash (it runs in `afterEach`, after every
  assertion) but it would under-reset a test that asserts after clearing.
