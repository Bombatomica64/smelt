# Erasure-Debt Audit — Generated es-toolkit Crate

*(Fable planning pass, 2026-07-12. Report run read-only with the release binary.)*

## 1. Classification + baseline status
`smelt smelt-unknown-report third_party/es-toolkit/dist-smelt/src --baseline blocker-logs/smelt-unknown-baseline.json`:
files 745 · runtime-prelude 1,968 · legitimate-boundary 36,525 · **avoidable-erasure 39,224**.
The committed baseline is the EXAMPLES corpus (27 files, avoidable=0) — comparing es-toolkit against it is meaningless; the methodology doc (blocker-logs/smelt-unknown-report.md:84-88) anticipated per-corpus baselines.
**Recommendation:** keep the examples baseline as a hard invariant (avoidable stays 0 there) and snapshot a second corpus baseline now:
`./target/release/smelt smelt-unknown-report third_party/es-toolkit/dist-smelt/src --format json --output blocker-logs/smelt-unknown-baseline-es-toolkit.json`
(record: 1,968 / 36,525 / 39,224 over 745 files; the classifier is conservative — ambiguous lines count avoidable — so this is a ceiling.)

## 2. Top 5 avoidable-erasure patterns (verified)
- **P1 — Generic type params monomorphized to SmeltUnknown (largest, ~3.5k+):** `chunk_1.rs:7` `chunk_14(arr: SmeltList<SmeltUnknown>, ...)` from `chunk<T>`; cascades into spec call sites erasing concrete literal arrays (2,235 occ/24 files + 838 temps/199 files). Origin is frontend generic instantiation (the emitter faithfully prints, call_runtime.rs:202-204). Recovery: scoped Rust generics (`fn chunk_14<T: SmeltValue>`) or per-call-site monomorphization. Widest impact; ROOT CAUSE feeding P2 and P5.
- **P2 — Placeholder default-callback thunks `Rc<dyn Fn(SmeltUnknown)->SmeltUnknown>` returning Null (~1,100+):** emitter/types.rs:1051 and :1139; the same mechanism emits concrete signatures when the frontend kept types (chunk_spec.rs:30-42) — the erased variant is P1 fallout. Recovery: propagate declared callback signatures; genuinely-erased ones belong in legitimate-boundary via the SmeltErasedFunction path.
- **P3 — Whole-object erasure at dynamic-property-assignment (~5,400 occ, ~8 files):** merge/cloneDeepWith/defaultsDeep/mergeWith — one computed-member-write lowering (frontend stmt/assignments.rs:1429/1450) erases the whole target, then every access pays the tax. Recovery: when the target is statically object/record, lower to `SmeltRecord::insert` directly; `Option<T>` for undefined checks. Cheapest fix-to-count ratio; unblocks P4.
- **P4 — Concrete-field record literals erased to `SmeltRecord<String, SmeltUnknown>` + `from_unknown_record` round-trips (~1,500):** emit sites coercion.rs:944 and :1301. Recovery: infer field types before defaulting to Unknown; empty-record-as-this idiom gets a one-shot boundary adapter.
- **P5 — Universal String/Array/Object index-dispatch match on statically-known receivers (~1,500, 87+ files):** frontend stdlib/objects.rs:636/:721 intern Unknown params; arms at :834/:1155/:1214/:1305 route known shapes through dynamic dispatch. Recovery: emit the single matching arm for list/record receivers; keep the full match only for true unknown/union (then it's legitimate narrowing).

## 3. Enforcement workflow
- Run in agent gates for any task touching frontend/MIR/codegen that regenerates a corpus, and in CI on PRs touching emitter code (textual scan, no compiler, deterministic diffs — cheap for every PR).
- Two baselines: examples = hard invariant (avoidable 0); es-toolkit = ratchet (avoidable may only decrease/stay equal).
- Policy wording: "Any PR that regenerates a corpus must include the report delta. avoidable(current) > avoidable(baseline) blocks merge unless the PR (1) documents the genuine dynamic boundary in a code comment at the emit site and (2) adds a regression test proving concrete types/unions/scoped generics cannot represent it — then reclassify via `classify_line` in crates/smelt-cli/src/unknown_report.rs and re-snapshot rather than accepting the increase. legitimate-boundary increases never block; avoidable decreases re-snapshot in the same commit."
- The tool currently always exits 0 ("advisory"); CI gating needs a `--fail-on-regression` flag or a delta-parsing wrapper.
