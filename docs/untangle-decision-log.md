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
