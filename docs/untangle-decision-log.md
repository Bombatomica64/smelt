# Untangle decision log

Skill: `caldoai/skills/untangle-codebase`, additions chosen: **Comments, Tests**.
Audited at `afcbce68` by six read-only auditors (TS stdlib, TS core, emitter,
codegen lib + runtime, HIR/MIR/specialize, tooling). Nothing in the audit was
compiled; PLAUSIBLE marks shapes confirmed in code with no source-level repro.
Status per fix is kept in the **Status** section at the end.

Measured: 383k lines Rust; `Rvalue` 223 variants, `ExprKind` 234, `Type` 22;
`writer.line` 4,803 in codegen (was 1,578 at the second-pass plan); `*_text` fns
471 (was 284); 399 `#[ignore]` (slow tier, run in CI); crate DAG is clean
(`codegen -> frontend` and `mir -> frontend-ts` are dev-only). The tangle is
inside crates, not between them.

The question for every fix: pieces that change together end up together, pieces
that don't aren't tied, everything depends on what changes less.

## R1. The IR owns its own traversal and per-op facts
One `reads`/`defs`/`reads_mut` API beside the types; each op states its facts once.
- `smelt-mir/src/opt/mod.rs:328-1205` `rewrite_rvalue`: third full copy of the operand walk (880 lines).
- `smelt-mir/src/validate/operands.rs:16,976`: canonical walker lives in the validator; `opt` and emitter depend on it for basic traversal.
- `emitter/core.rs:6331` `rvalue_uses_local`: fourth walker, 70/223 variants then `_ => false`; f90a39ca records this fallthrough dropping a closure decl. PLAUSIBLE live.
- "Assigned once" has 3 disagreeing answers: `opt/mod.rs:219`, `opt/local_use.rs:43`, `move_on_last_use.rs:323,340`.
- `opt/local_use.rs:131` ignores `Callee::Indirect`, others count it.
- `operand_local` x6, `operand_place` x2, `single_use_temp` x2, `is_temp_local`/`local_is_temp`, `place_reads_local` x2.
- In-place mutation target list x3 with confirmed drift: `literals.rs:42`, `local_analysis.rs:541`, `classify.rs:71` (`ListUnshift`, `ListSplice`, `ListSort`...).
- `Type` child walkers hand-written ~12x (`generic_bindings.rs:861,911`, `classes.rs:185,1307,1383,1826`, `generic_elision.rs:368`, `typedness.rs:575`, `union.rs:91,227`, `core.rs:5585`); 3 substitutions (`generic_bindings.rs:364`, `classes.rs:2114`, `types.rs:2544`).
- A new op touches 11-18 files; same 10 every time: `kinds.rs`, `map.rs`, `format/call.rs`, `lower/expr.rs`, `lower/place.rs`, `types.rs`, `mir/format.rs`, `opt/mod.rs`, `operands.rs`, `call_runtime.rs`.
- `lower/place.rs:300-550` `place_unsupported` lists ~230 kinds to reject them.
- Fallibility split across `BuiltinFn::is_fallible`, `UriTranscodeOp`, `HostModuleOp`, `lower/mod.rs:829`; `emitter/call.rs:838,3152` two dispatches.
- Emitter cannot obtain a type unless pre-interned: `lower/mod.rs:796,829,232`, `passes/generic_records.rs`, `passes/inherited_types.rs`.
- One-idiom op `RegexReplaceFirstMatchUppercase` beside general `RegexReplaceCallback`.
- `opt/dict_entry_mutation.rs:25`: `ListPush` with `Place::Index` silently means in-place.
- `integer_range.rs:96,109`, `lower/context.rs:339,484`: `absent` used as a language flag.

## R2. Surplus goes
- IR no one produces: `Phi`, `StorageLive/Dead`, `Terminator::Unreachable`, `Rvalue::Conditional`, `Rvalue::JsonParse`, `ExprKind/Rvalue::ListRepeat`, `Pattern::Wildcard/Literal`. (`Pattern::Tuple` kept: possible feature gap.)
- `smelt-stdlib` `rules.rs:1-105` shape enums, `diagnostics.rs`, `DECODE_URI_OCTETS`, test-only `backend_dependency`/`source_api`/`host_module_dependencies`, `unmodeled_package_use_blocks()` constant.
- Crates with no dependents: `smelt-python`, `smelt-test`; `smelt-asyncio` 42 lines one caller.
- `specialize`: `NativeAdapterRegistry`, `OciSandboxBackend`, `load_or_compute`.
- `opt`: `Pass`/`FinalPass` duplicates, `name()`, public `Pipeline` fields.
- `py-types` `resolved_return_count/param_count`, `is_recognized_pytest_api`, `smelt_missing_index_value`, `rvalue_needs_regex(_mir)`, `any_rvalue_needs`, `emit_crate_with_modules`, `type_text_with_impl_trait` flag (90/91 false), `has_dynamic_shape` x4 aliases, `ReportFormat` x5 aliases, wrappers listed in the audits.

## R3. Runtime demand is computed once; the runtime is real Rust
- Hand-kept gate closures: `stdlib.rs:1206-1368`, `lib.rs:1372`, `1113`, `stdlib.rs:1145,1183`, with E0425/E0433 scars at `stdlib.rs:731,1152,1168,1251,1256`.
- PLAUSIBLE defect: generators/timers/structuredClone/typeof nested in `if needs_unknown` with no forcing clause; `Generator<number, void, void>` may not compile.
- Scan drift: `rvalues()` vs `stdlib.rs:1320-1368` vs `needs_tokio`; whole-MIR rescans during rendering.
- Value core still text: `lib.rs:1481-6016`, 1,989 `writer.line`, hottest region (361/703 hunks). Three spellings of the runtime.
- Emitted duplicates: `lib.rs:1454/2430`, timers `5109,5120,5226`, `typed_array_prelude.rs:311,634,970`, `fetch_types_prelude.rs:1223/1266`.
- Three naming systems for runtime symbols; `__smelt_*` 81 markers/674 mentions; `unknown_report.rs:681` 36 raw markers.
- Text patching: `lib.rs:7020`, `7040`, `with_bigint_arm` `8925` (debug_assert only).

## R4. "Runtime prelude" means runtime
- User interfaces/classes/unions/JSON decoders (`lib.rs:6018,6551,6794,6804`) are emitted before `PRELUDE_END_MARKER`, and `unknown_report.rs:416` counts everything before it as prelude: the avoidable ratchet may undercount. Metric integrity.

## R5. The emitter keeps structure instead of re-reading text
- `names: HashMap<LocalId,String>` holds rendered exprs; `"(*smelt_capture_"`, `closure_arg_`, `self_owned` parsed back; `rendered_text_rewrite.rs` exists for it.
- Sentinels: `SmeltUnknown::Null` place write dropped (`control_flow.rs:410,887`), `"Default::default()"`.
- `ERASED_CARRIER_TOKENS` (`core.rs:6656`) decides genericity by text search; `trial_fixed_point` 16 rounds.
- `RenderedValue` precedence is a no-op at the seam (both `erase_value` callers pass `atom`).
- Rendered-text sniffing: `?`, `(Vec::new())`, `&`, ` as f64`, `.clone()`, `SmeltUnknown::`, `"return Ok"`, 10 sites comparing to `"SmeltUnknown"`.
- Ladder copies: `core.rs:4123/4570`, `coercion.rs:103/988`, `erase`/`erase_value`, `core.rs:458/2740`, `control_flow.rs:218/257`.
- Host-class facts in strings: RegExp `types.rs:1000`/`call_runtime.rs:2614`, `instanceof` `call.rs:3594-3656`, `Date`/`File`/`DataView`.
- Name rules: `core.rs:861` `index*`, `call.rs:2726` `piped_`, `call.rs:2710` `(fn_)`, `optional_access.rs:28` `groups`, `MatchFnResult`, `call.rs:998` `concat`.

## R6. Built-ins are recognised once, from one registry; packages are data
- Silent miscompiles from library-spelling stubs: `objects.rs:777` `_.has` -> false, `objects.rs:811` `fp.map` -> None, `numbers_math.rs:1184` `process.version.match`, `:1215` `process.cwd()` -> "", `classify.rs:751-856`, `list_ops.rs:754,845`, `references.rs:372` `strapi`, `arrows.rs:699` `convertToFP`, `annotations.rs:2452...` remeda types, `dispatch.rs:222`, `annotations.rs:4194` `flush`, `tz` marker across 13 files.
- Triple recognition: `call_dispatch.rs:5129`, `stdlib.rs:1504,1513`; 142 name comparisons; 20/89 `RuleId` unused; ~6 definitions of shadowing; Python has no shadow check (`stdlib_dispatch.rs:9`) and matches spelled module not binding.
- ~23 `let Ok(receiver)=... else None` swallow errors and re-lower; handler order "load-bearing".
- Lists copied: error classes x6, Math x3, `Number.MIN_VALUE` x2 (bug: `f64::MIN_POSITIVE`), `RequestInit.signal` drift, `GLOBAL_ALIAS_NAMES`.
- Source-text probes: `call_dispatch.rs:2715,2761`, `annotations.rs:5026`, `module_init.rs:4879,5172`, `operators.rs:4130,4149`.
- Phase 5: `ListPop` Optional vs T, `is_json_serializable_type` x2, `SmeltError` x2, specialization path x2.

## R7. Callback bodies lower through `expression`
- `callbacks/dispatch.rs:1023` 1,459 lines re-implementing expression lowering into 24-variant `CallbackExpr`; `closures.rs:737`; `dispatch.rs:1777`; `dispatch.rs:921,761`; 318 mentions.
- Return-type policies x3: `arrows.rs:606`, `body_lowering.rs:1522`, `functions.rs:794`. Rollback by hand: `module_init.rs:2823`, `guards.rs:1921,1934`, `asserted_member_read.rs:72`.

## R8. One lowering driver, one report shell, a real library
- 4 drivers: `lowering.rs:84,164,443,939`; the blocking typedness ratchet measures one `smelt build` never uses.
- `smelt-transpiler/src/lib.rs` is a comment, so `check --lib` never compiles the 14k-line CLI.
- `pipeline.rs:54-83` and `smelt-gui/src/compiler.rs:279` copies; add `smelt_mir::compile`.
- 5 `ReportFormat` aliases, manifest parse x4, gate protocols x2, baseline loaders x2, `escape_cell`, `push_fmt` x3, `bump` x3, `share` x3; Markdown wording parsed as API.
- Source classification: ext lists x4, walkers x2, `starts_with("dist")`; `path_matches_glob` in volatile `lowering.rs` creates module cycles.
- Manifest: `[rust] edition/dependencies` documented, ignored; `clone-strategy`, `[strict]` required, unread.

## R9. Gates cannot pass vacuously; CI runs from data
- `--fail-on-regression` without baseline exits 0; missing baseline file skips; examples invariants label-gated on PRs; `es-toolkit-ci-baseline.json` `probe_blockers: 9`; `typedness_report.rs:174` exact `Smelt.toml`.
- `probe.rs:372` `unwrap_or((0,0))`; `ts_facts.rs:55` `.ok()`.
- No composite actions; pins outside `libraries.json`; 79 tiers by hand; `coverage_badge.sh` `region_pct="$line_pct"` (false badge); `lcov.info` 298 revisions; `publish-crates.sh` lists non-member `smelt-gui`, omits `smelt-python`; `justfile:114` `$$crate`.
- 60 of 257 blocker-logs and 11 docs referenced by nothing; `third_party/strapi` 83 MB.

## R10. Code lives where its concern lives
- `testing/matchers.rs:2412-6117`: narrowing/declarations in the assertion file. `call_dispatch.rs:4536-6127` overload resolution. `stdlib.rs:606-1400` vitest. Math in `objects.rs`, EventEmitter in `fetch_types.rs`, Date/timers in `guards.rs`, statements in `types_iface.rs`.
- `codegen lib.rs` 12 concerns, table in audit 4; `lib.rs:8654-9120` coercion outside the seam; `classes.rs:247-1736` generic lifting. `CrateBuilder` order doc-only.
- `HirCtx` cloned ~15 maps per builder; `with_lexical_scope`; symbol source spelling (26 fallbacks).

## R11. Bugs surfaced (fixed where the structural fix reaches them)
- `Number.MIN_VALUE` folds to 2.2e-308. `matchers.rs:1777` `toBeInstanceOf` can never fail. `rendered_value.rs:181` test cannot catch its target.

## Comments (chosen)
Restating ~3-4%; 1,539 `///` lines emitted into every generated crate; history narration ("used to", "was E0...") and stale comments are the larger share.

## Tests (chosen, done last)
`thrown_tests.rs` contains-checks match the definition (39/72); `a_missing_runtime_item_is_an_error`; `detect_regression_requires_baseline` pins a vacuous gate; `hir_cli_*` silent `return Ok(())`; `part04_tests.rs:120,146,2389,6975` validate-only pins of stubs; 57 negative-only contains; harness copied into 79 files; `part_7_tests.rs` 15.5k lines.

## Found while verifying
- **Emitted output is nondeterministic.** `callbacks/dispatch.rs:1960` iterates `object_namespaces: HashMap<String, HashMap<..>>` into match arms, so std's per-process hash seed picks the arm order: unmodified `smelt-base` flips `"p"`/`"P"` in date-fns `format_index.rs` between runs. Arms are equivalent, but files rewrite and Cargo loses incremental reuse. `clippy::iter_over_hash_type` finds 20 `for` loops in `smelt-frontend-ts`; non-`for` uses (`dispatch.rs:2377` `.keys().collect()`) need a manual pass.
- **ts-pattern lowering exceeds 12G** on its own (oom-kill at the cap, serial run, unmodified binary). The byte oracle covers es-toolkit, remeda, radash, hono, date-fns.

## Status
- R2/R8/R9 first pass merged (`bbf5831d`): surplus, `smelt-transpiler` lib target, `report`, `source_files`, gates. Corpus oracle: only the two hash-order date-fns files differ, and base flips them too.
- R1 traversal slice + R2 dead IR merged (`5ad1fddc`): `traversal.rs`, `Rvalue::mutated_receiver`, `smelt_mir::compile`, one assignment/read count. Corpus oracle: 0 of 3,104 rows differ. Fixed `fs.push(closure)` E0425.
- Removed by decision: `third_party/strapi`, 68 unreferenced logs/docs, `smelt-test`, `smelt-python`, `lcov.info` from git.
- Base is red on main at `afcbce68`: `corpus`, `coverage`, `hono`, `radash-regression`, `remeda-regression`. Needs its own stream.
- Follow-ups from R1: `splice` with a closure hits E0271; `Object.assign(this.data, o)` printed 0 where 1 is expected (same at base); dead `ListCallback` closure bindings; `specs/mir.md` still describes `Phi`/`Storage*`/`Unreachable`.
- R3/R4 merged (`6ad0586f`): `RuntimeDemand`, value core and preludes are compiled Rust in `smelt-runtime` (176 items), `writer.line` 4,836 -> 84, one owner for markers, program types after the prelude marker. Fixed `Generator<number, void, void>` E0425. Avoidable: examples 0 -> 1,020, es-toolkit 17,286 -> 17,742, a move of hidden lines rather than new erasure; the 1,020 are adapter and JSON-decoder bodies, with no struct field, type declaration or ordinary signature among them.
- Open bug: `routers[0].match('p')` on an interface lowers to `String.prototype.match` (silent miscompile). A fixture was renamed to `route`, so nothing covers this now; needs a fix plus its own test.
- Open: emitted `extract_if` needs Rust 1.88, workspace declares 1.85. `cargo clippy --all-targets` fails on `discriminated_union_tests.rs`. Byte-changing duplicate collapse from R3 still pending.
- R6 merged (`c0b7ccd5`): output deterministic (two full oracle runs of the merged binary, 0 of 3,102 rows differ), stubs deleted, one built-in registry, `SourceFacts`, one recognition lookup, `toBeInstanceOf` real. Cost of deleting stubs: date-fns excludes `intlFormat`, `intlFormatDistance`.
- Ratchet resolved by decision (`eab738f7`): adapter bodies are boundary. Avoidable: examples 0, es-toolkit 17,348, remeda 20,086. es-toolkit's +62 is real program storage previously hidden.
- Open from R6: `class X extends Error` is not `instanceof Error` in generated code, 4 es-toolkit specs; `isFile_spec` `File` instanceof, 1 spec. `forward_arrow_const_names` (module_init:~5172) keeps substring matching until the closure path is fixed. `instanceof` host list in `guards.rs` not unified. `user_class_shadows`, `module_type_shadows_global` separate. No lint guards `.keys()`-style hash iteration; `smelt-mir` and `smelt-codegen-rust` not swept. `NonEmptyArray` lost by-name min-arity. Still silent: `import * as _ from "lodash"` member calls, `c.flush()` on a class without it.
- Red main fixed (`a19597fd`): all failures traced to #310 by diff reading. Full suite 3,308 passed, 0 failed, 409 ignored, on the pre-R6 base; remeda 1789/0, radash 383/4 (pin), hono check clean, es-toolkit 1048/11 at that base.
- Open from red: `namespace_member_call` does not instantiate generics as a direct call does (`_.iterate(..)` types the closure at `&SmeltUnknown`); hono's advisory test crate has 6 E0308 in `router_test_2.rs`, predating this work, so hono runtime tests do not run; `heapSiftDown<T>` calling `swapInPlace(unknown[])` copies per swap, O(n), unbenchmarked; `callback_generics` drift note prints "observed 0".
- R7 done on `t3/retire-callback-expr-miscompiles`: callback bodies lower through `closure_body_expr_from_parts`; `callback_expression`, the compact statement IR, the name-keyed re-dispatch and `LocalCallback` are gone. A name or callable value is a `ForwardingCallback` (`callbacks/forwarding.rs`). `CallbackExpr` left `smelt-hir`; Python keeps a private trimmed copy. One return join (`joined_inferred_type`), one speculation guard (`SpeculationMark`), one shadow rule (`source_shadows_global`), `with_lexical_scope`. `forward_arrow_const_names` reads `SourceFacts`.
- Open from R7: classes are not first-class values (`{ c: Oops }` lowers `c` to null, `error instanceof pair.c` answers false); a function parameter named like a module object constant empties that constant at reads (`const table = {..}` with `f(table)` elsewhere); `.replace(re, () => undefined)` substitutes `""` where JavaScript substitutes `undefined`; `class B extends A implements R` rejects a method inherited from `A`; `console.log` of an erased-generic list prints `[Number(3.0)]`; a local `const Base = cond ? X : Y` is not accepted as a base class.
- R7 merged (`1f9fbc7a`): `CallbackExpr` retired from HIR and the TS frontend, one return join, `SpeculationMark`, `with_lexical_scope`, `SourceFacts` everywhere, `instanceof` via registry edges; `class X extends Error` and `File` instanceof fixed, `match` on interfaces fixed and its fixture restored. es-toolkit 1048/11. Python keeps a private trimmed `CallbackExpr` copy (surplus, to remove).
- CI on `d0db0875`, pre-R7: all jobs green including `es-toolkit-regression`, `remeda-regression`, `radash-regression`, `hono`.
- Decision: es-toolkit typedness rose in 9 programs (corpus erased -55 overall). Remove redundant temporaries at source first, then re-snapshot the remainder with evidence. Decision: `[strict]`/`clone-strategy` follow specs/cli.md (hard errors); `[rust]` keys implemented fully.
- Corgi store hit 176G: each stream that touches `smelt-hir` invalidates everything above it. Wipe authorised below ~30G free for this task.
- R5 on branch `t3/emitter-text-rewrite-cleanup`: `LocalBinding`/`PlaceTarget`, `RustShape`, one `Type` child visitor and substitution, carrier events for the generic trial, `CallText`, shared ladders, host facts in `smelt_stdlib`. Steps 1-6 byte-identical; step 7 changes es-toolkit `trim.rs`, remeda `chunk.rs` and `piped_test.rs`.
- Open from R5: `cloned_value_text`/`borrowed_value_text` still read text (now in `rendered_value.rs`); removing them needs every `*_text` producer to return a `RenderedValue`. `note_type_erasures` keeps records for renders the output never spells. `value_at_type_in`/`value_at_type_text` remain two ladders.
- Open bug (frontend): `.value` on an `Optional` lowers to `iterator_value` whatever the inner type. `if (this.b) return this.b.value` and `const x = b as Box; x.value + 1` read the whole optional; the second emits `Option<Box_> + 1.0`.
- Open bug: `Array.isArray(chars)` narrowing of `string | string[]` then `chars.flatMap(..)` emits an empty `SmeltList::default()`, a silent miscompile (es-toolkit `trim` with array `chars`; Node prints `a-b-c`, generated prints empty).
- Open: `Intl.DateTimeFormat` lost its stub in R6, so `emits_date_to_iso_string_for_erased_datearg_surfaces` fails in the frontend, before codegen.
- R5 merged (`66887d84`): `LocalBinding`/`PlaceTarget`, `rust_shape`, one `Type` visitor and substitution, carrier events replacing the token search, `CallText`, shared ladders, host descriptors; name rules deleted. `rendered_text_rewrite.rs` deleted, ~130 lines of text reading moved to `rendered_value.rs`. Oracle after the hand merge: exactly the 3 intended files differ of 3,100 rows.
- Partial from R5: `*_text` 496 -> 502 (finding D's full migration not done), `value_at_type_in`/`value_at_type_text` still separate, `RenderedValue` precedence not carried, three text checks left (`erase_concrete_union_text`, `core.rs:686`, `emitted_tail_returns`), spurious erasure records kept by `note_type_erasures`.
- Open miscompiles: `iterate(2, handler, 0)` with a callback returning `unknown` prints NaN, Node prints `0-1-2`: `T` is not widened at the call site. The red-main fix only changed the adapter, and its test pinned text. R7 also made a `const` with a declared function type adopt the arrow's inferred return. `.value` on an `Optional` lowers to `iterator_value`. `flatMap` after `Array.isArray` narrowing of `string | string[]` emits an empty list.
- Miscompile round on `t3/fix-silent-miscompiles`. Rule for a generic call: every candidate position counts, read off the checker's answer for the call where it has one, else the widest lowered candidate. The result only ever widens: `fork([], f)` is `never[]` to the checker, and a narrower claim describes a value the definition does not return. A binding's annotation is the binding's type (`unknown` included); the arrow's own return no longer replaces it. An iterator result is `Type::GeneratorResult`, never `Optional`, so `IteratorDone`/`IteratorValue` and the emitter guard are gone. `inferred_default_text` is gone: its sites are hard errors, except `flat`, where an item that is not an array stays. A record's `Default` no longer stands in when a program lacks the adapter's types, and an index or tuple read of a narrowed optional union reads the value instead of `undefined`.
- `namespace_member_call` still returns the declared `T`: Node agrees, since the erased value is the real one. Instantiating it there broke radash three ways, each a direct-call bug too: `range` (a generator's `T` reachable only through a union), two block-local `class Data` collapsing into one, and `clone(array)` answering an object because `new arr.constructor()` is not an array.
- Open from that round: es-toolkit avoidable 17,346 -> 17,387 (baseline 17,348), real reads where a constant `undefined` or an empty list stood, so `--fail-on-regression` blocks until they are reclassified. With `SMELT_TS7=0`, an inline arrow with an explicit `: unknown` return still infers its body's type. `member_call`'s unresolved method still lowers to `undefined`. `[...(x as Set<number>)]` of a hash-backed set loses insertion order. `Object.keys` on a narrowed optional union is a hard emitter error.
- Spread bug traced (predates this session; unmodified binary prints `1 1`, Node `1 3`): a spread into a function value's rest parameter delivered one element instead of N when the list's element type differed from the rest's. `rest_spread_list_expression` left a concrete list unconverted, so `List<Float>` reached a `List<Unknown>` slot and the emitter, which guesses whether packing already happened by comparing exact types, packed it again. Fixed at source with an element-wise conversion.
- Open, same family: rest packing still has two owners. Frontend packs, and `packed_rest_arg_text`/`rest_vector_call_args_text` in the emitter guess. One owner means moving the remaining unpacked frontend routes to pack, then deleting the emitter side. Repro of the remaining failure, a compile error: `type S = (...args: never) => unknown; function v(fn: S, args: readonly unknown[]) { return (fn as (...a: readonly unknown[]) => unknown)(...args); }`: the `as` on the callee is dropped, so the emitter compares against `List<Never>`.
- R8 done on `t3/untangle-r8-typedness-manifest`. Optimizer: closure bodies are optimized, `TempCopyForwarding`, `SharedLocals`, counts once per body. Evaluation order: `VolatileLocals` snapshots a read a sibling can invalidate, stores in value position evaluate in place. Driver: one `ManifestLowering`; `smelt build` output did not move, 146 dump goldens did. GUI through `smelt_mir::compile`, its crates patched to this repository. Manifest: `[rust]`, `[project]` reach `Cargo.toml`, any edition builds, `deny_unknown_fields`, spec-worded `smelt::manifest` errors. Reports: `TestReport` with Markdown and JSON renderings.
- `test-prefix` stays an alias: the frozen corpora spell it. 43 manifests are tracked, not 52.
- Open from R8: es-toolkit typedness baseline not re-snapshotted, pending the decision on rises with no wrong-behaviour evidence. `flatMap` in es-toolkit compat emits `flatten_depth(None, Some(1.0))` for `flattenDepth(mapped, 1)`, dropping the list. `obj[key]` with a string key on the `Array` arm of an erased receiver emits `f64::NAN as i64`, reading index 0. `pair(a, (a = 2))` is rejected as a call argument. A closure parameter written by a nested closure fails with E0425 (`smelt_capture_closure_arg_0`). Three runtime tiers need 2.0-2.5 MB of stack locally, over the 2 MB test-thread default; CI passes them. `corgi` store at 105 GB.
- #312 and #313 merged. Stage 2: one `ManifestLowering` driver, manifest keys, `TempCopyForwarding`, evaluation order, `memoize` typed via a `vi.fn` declaration. Full suite 3,383 passed, 0 failed.
- es-toolkit baselines re-snapshotted by the user's decision: typedness erased 8650/1087/1299 -> 8574/1079/1300, avoidable 17,348 -> 17,350. Debt: conversion temporaries in `flatMap`, `zip.spec`, `cartesianProduct.spec`, `without`, `toDefaulted`, `xorBy`, `xorWith`, `transform`, `matches`; the +2 avoidable is unexplained.
- Open from stage 2: compat `flatMap` passes `None` for its list and `obj[key]` with a string key on an array reads index 0 (both from reading emitted Rust, not run); `(a = 2)` as a call argument rejected; a closure parameter written by a nested closure gives E0425; `declared_erased_return` failed once in a parallel run; `clippy --all-targets -D warnings` fails locally (633 in `smelt-runtime`) while CI passes; editions 2015/2018 never built.

- R1 ops on `t3/rvalue-op-table-unify`: one X-macro table (`smelt-hir/src/op_table.rs`, 206 ops) generates `ExprKind`, `Rvalue`, walkers, dump text and transcribed lowering. A new op is 3 files (table, emitter arm, frontend), was 9-11. Facts per op: `fallible`, `call`, `mutates`, `fresh`, `needs`, field `@ROLE`. `lower_expr` 3,720 -> 1,291 lines. `BuiltinFn` is the console family only; a throwing op is `Callee::Op`, whose `args` are empty, so read operands through `Terminator::reads`.
- Kept by measurement: `generic_records`, `inherited_types`. Unions and record-helper ordinals come from `TypeInterner::all()` before any body renders, so deleting them leaves names undeclared. Examples stayed byte-identical; date-fns went E0412 (3 unions) and only the corpus oracle saw it. `a_union_only_an_inherited_copy_spells_is_declared` pins it. Finishing this needs the 14 `all()` scans to become demand-driven. `lower/mod.rs:232` interns the types of real locals and stays.
- Open: `TempProject` (`tests/common/mod.rs:88`) names its directory from pid + nanoseconds, so two parallel tests can share it. Seen once: `node_parity` ran another fixture under Node. Fix: add an `AtomicU64` to the name. `extracts_borrowed_predicate_argument_from_dynamic_spread_dispatch` is red at `4a8d63a3`: `impl(...args)` passes a no-op predicate `&|arg0: f64| false`. A generic record field `T | boolean` emits E0308 (field erased, constructor builds the union).
- Op table merged (#315): a new op touches 3 files, was 9-11. Pre-interning partly removed; `generic_records` and `inherited_types` stay until the 14 `all()` declaration scans are demand-driven.
- CI on main at `93fe58fb`: all jobs pass except `coverage`, on `generic_call_chain_scales_linearly` (growth 10.56, bound 10). Uninstrumented growth is 7.3-8.8 over 5 runs, so instrumentation tips a thin margin. Codegen is the superlinear part (about 7x on 4x input). Bound not relaxed.
- Open from op table: `TempProject` (`tests/common/mod.rs:88`) names dirs from pid + nanoseconds, so parallel tests can collide, likely the intermittent parity failures; generic record field `T | boolean` gives E0308; `DictEntryPush` in `mutated_receiver` pinned by unit test only.

- Python lambdas lower through ordinary expression lowering (`lowering/closure.rs`); `callback_expr.rs` is gone. `/`, `//`, `%` are `PythonDivision`, `d[k]` is `PythonDictRead` (raises `KeyError`). Open: `try` around `map`/`filter`/`sorted` whose callback raises does not catch it, in TS too (the op stays an assignment, so `catch_unwind` wraps the wrong call); `print` of a `list[str]` prints double quotes; list index out of range does not raise; hint-less `return lambda ..` rejected; `map(module_fn, xs)` rejected.
