# es-toolkit final 45 — architecture plan

Baseline: **1014 passed / 45 failed** (`blocker-logs/estk-current.md`, branch
`claude/estoolkit-test-failures-4fuf9e` at `b149994`). Six read-only investigations
(`estk-final45-{arrays,functions,objects,predicates,misc,async}.md`) root-caused every row
against the generated Rust. Result: **43 general defects in 29 root families, 2 true host
gaps.** Three rows earlier filed as "out of scope" (`debounce` spyOn, `throttle` this,
`isBuffer`) were re-verified and are fixable; the prior rulings rested on premises that are
no longer true of the emitter.

## Out of scope (2)

| test | why |
| --- | --- |
| `isBrowser should return true in browser environment` | `// @vitest-environment happy-dom`; profile is non-DOM by construction, `isBrowser()` correctly folds to `false`. |
| `isPlainObject should return true for cross-realm plain objects` | `node:vm` `runInNewContext('({})')` = JS eval in a second realm. |

## Deferred, needs a decision (1)

| test | why |
| --- | --- |
| `at should return undefined for non-integer indices` | `at<T>(arr: readonly T[], indices): T[]` — an out-of-range `arr[i]` read yields `Default::default()` for a concrete `T` (`String::new()`), `Undefined` only for erased `T`. Honest fix is `noUncheckedIndexedAccess`-style typing of unproven index reads as `Optional<T>` (L, cross-cutting). Not started in this pass. |

## Root families → implementation batches

Two builders run at a time (CLAUDE.md cargo cap). Each batch is one Opus agent in its own
worktree, sharing `CARGO_TARGET_DIR=/home/user/smelt/target` (deps shared, workspace crates
keyed by path). Rounds are sequential; each round starts from the merged state.

### Round 1

**Batch A — frontend-ts: truthiness, name special case, non-null, await** (6 tests)
- F1 truthiness: `callback_truthy_expression` lowers `!unknown` to `UnknownIs{Bool}` (a typeof test); `lowered_condition_expression` folds `TypeParam` conditions to `true`. Add a callback truthiness node lowering to `PrimitiveCast{ToBool}`; narrow the `Function|Class|TypeParam ⇒ true` arms. → compact, dropWhile, dropRightWhile, negate (2nd defect).
- F2 delete `lodash_negate_call` (name-keyed special case replacing a user's `negate` with `null`); audit `lodash_has_call`, `lodash_fp_curried_call`, `lodash_for_each_call` for the same shadowing. → negate.
- F25 `x!` argument into an `Optional` parameter hint must stay a no-op, not `Some(x.expect(..))`. → randomInt.
- F27 `await <erased non-Future>` must not lower to `null` with the operand dropped; drop the `type_hint.is_some()` gate, `await v` of a concrete non-future is `v`. → attempt.

**Batch C — emitter: nullish representation, list aliasing, own keys, method identity** (7 tests)
- F7 undefined-in-containers: array-literal element hint must widen to `Optional` for a nullish element (zip expected side); erased→`String`/`Number` extraction must not coerce `Undefined` to `"undefined"` (zip actual); Dict erasure needs the `Constant::Undefined` recovery the List path has (isJSONValue undefinedProperty); absent property read on a class receiver is `Undefined` not `Null` (`place.rs:483`, cloneDeep `#b`). Interim for mergeWith: the `{call; SmeltUnknown::Undefined}` void-adapter shortcut (`core.rs:3862`) must not fire when the callback body returns a `null` literal — only if clean; the principled `Type::Null` split is L and out of this pass. → zip, isJSONValue(undefined), cloneDeep instance, mergeWith(stretch).
- F5 `(List,List)` coercion with identity element map must alias (`with_storage`) not rebuild (`with_id` + collect); compare Rust renderings not TypeIds; by-ref parameter must never receive a rebuilt buffer. → remove.
- F9 own-key enumeration on `SmeltJsMap` must drop `__smelt_proto:`/`__smelt_method:`/`__smelt_class` markers and map `__smelt_symbol:` back (prelude helper used by Keys/Values/Entries/ForIn). → invert.
- F11 class method reference reads need one canonical identity per (class, method) via `smelt_link_function_identity_key` (both `class_method_reference_text` and `class_proto.rs`). → clone custom classes.

### Round 2

**Batch B — frontend-ts: overloads, spread hints, intersections, symbol interning** (7 tests)
- F4 tuple/non-empty-array parameters need call-site length evidence (`param_min_len`, mirroring `min_rest`), so `readonly [T, ...T[]]` and `[A,B]` overloads stop swallowing plain arrays. Also: a callee whose lowered return is erased/`Optional` keeps that at the call (precedent `call_dispatch.rs:934`), no `map_or(Default::default())` collapse. → initial, maxBy, minBy, reduceAsync.
- F16 array-spread item hint that is the callee's out-of-scope `TypeParam` must be dropped (unify piece types); `list_concat_text`'s two `Default::default()` fallbacks become `EmitError`. → sumBy.
- F13 `A & B` over records lowers structurally (merged members, per-field union), never `Type::Union`; union recovery must discriminate object arms structurally and never retype leaves. → toMerged.
- F3 `intern_source_name` case-folds (`Foo`/`foo`, `Buffer`/`buffer` share one `Symbol`, last-writer-wins spelling). Member/property names intern exactly (`intern_exact_source_name`) or the interner separates source spelling from Rust ident. → intersectionWith (+ unblocks isBuffer key).

**Batch D — runtime object model** (6 tests)
- F8 `SmeltList` gets a lazily allocated named-property side table; `smelt_index_assign` and the static store (`control_flow.rs:535`) insert there instead of replacing the array; reads/`Object.keys`/`for-in` consult it; structural equality ignores it. → merge, isEqualWith non-index.
- F12 `Object.prototype` members as a lookup *fallback* table (never entries): `smelt_get_object_field` and the `__smelt_proto:object` sentinel fall back to it; `in`/`toHaveProperty` consult it; enumeration/equality/JSON never see it. → toSnakeCaseKeys.
- F21 `Symbol.<wellKnown>` in value position is `Literal::Symbol`, one shared table maps spelling↔storage key (`well_known_symbol_key` + `smelt_property_key`); `smelt_object_to_string_tag` honours `@@toStringTag` first. → isSymbol, isPlainObject(toStringTag).
- F18 `host_base_markers` stamps `__smelt_error: "<nearest builtin error base>"` for a class whose chain reaches a builtin error. → isError.

### Round 3

**Batch E — host globals / predicates** (6 tests)
- F17 a module const whose initializer folds to the global alias is a global alias, and an imported binding of it too; `typeof <Unknown|Union|TypeParam> !== 'undefined'` emits `UnknownIs{Undefined}` not a constant; `Buffer.from(string)` encodes bytes. → isBuffer, isEqualWith buffers.
- F19 erased class constructors register class identity; `instanceof <host name with override slot>` reads the slot (`Native` ⇒ marker probe, `Ctor(f)` ⇒ class chain, `Absent` ⇒ false). → isFile ×2.
- F20 `<Builtin>.prototype.<method>` (and `<Builtin>.<fn>`) as first-class callable values via the stdlib registry (generalizing `object_static_function_member`). → isFunction.
- F10 `new String(x)` boxes like `Number`/`Boolean` (`__smelt_string`); string member reads unbox. → cloneDeep String objects.

**Batch G — stdlib throwing, reflection, regex, vitest** (5 tests)
- F22 `JSON.parse` lowers through `Terminator::Call{Callee::Builtin, unwind}` like `Await`, runtime `smelt_json_parse -> Result`, `may_throw` propagates. → isJSON.
- F23 `Reflect.ownKeys` → new `DictProjectionOp::OwnKeys` (strings then symbols, `List<Unknown>` — a genuine `string|symbol` boundary). → isJSONValue symbol key.
- F24 JS→Rust regex translation as a class-aware scanner (no literal `str::replace` hacks), untranslatable pattern is loud; `replace_string` expands `$$ $& $\` $' $n $<name>`. → escapeRegExp.
- F26 asymmetric matchers as marker values + `smelt_asymmetric_match`; `toEqual`/`toStrictEqual`/`toHaveBeenCalledWith` deep-equal consults markers. → sampleSize.
- F29 `vi.spyOn(target, name)` becomes a real boundary adapter: resolve current member (field else synthesized host method via one shared prelude helper), wrap in `smelt_vitest_mock_new(Some(original))`, insert under `name`, restore table. → debounce.

### Round 4

**Batch F — function semantics** (5 tests)
- F14 `recv.m(args)` lowers as `ClosureCall{ BindThis{ Field{recv,m}, recv } }` when the callee type can observe `this` (`Function|Unknown|TypeParam|Union`); arrows unaffected; no `this` ⇒ no channel. → memoize, throttle.
- F15 non-arrow functions have a `prototype` own property; `new f(args)` on a function value is a real `[[Construct]]` (`Rvalue::Construct`), `instanceof <function value>` walks `__proto__` against `target.prototype` instead of folding to `false`. → partial, partialRight.
- F28 a `const` read by an earlier closure in the same body is predeclared (generalize `predeclare_local_arrow_callbacks`); `source_contains_forward_callable` never fabricates a value for a function-local name. → withTimeout.

## Validation per batch
`cargo check --lib`, `cargo clippy --lib`, focused unit tests, new regression test(s), then
`smelt rust-test-report --focus <spec>` on the batch's es-toolkit specs plus `--guard` on the
sibling specs. Final validation after all merges: full es-toolkit report with
`--baseline-report blocker-logs/estk-current.md`, remeda 1789/1789, radash 84/84,
`smelt-unknown-report` es-toolkit ratchet, `cargo clippy --all-targets`, `cargo test`.
