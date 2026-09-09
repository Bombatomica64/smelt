# The example goldens cover every generated file (round 28, item 1)

## What was wrong

`verify_end_to_end_example` compared `dist/src/main.rs` against `expected.rs`.
A program whose lowering splits into modules puts its user code in
`source_<entry>.rs` and leaves `main.rs` holding the runtime prelude plus a
`mod` declaration — so for every fixture in that state the golden checked the
PRELUDE and nothing the fixture was written to exercise.

**34 of the corpus's 79 fixtures were in that state.** A feature could change
its own emitted Rust, in the file the fixture existed to pin, with every golden
still green. The stdout tier still ran the whole crate, so behaviour was
covered; the emitted SHAPE was not.

## The fix

`expected.rs` is now every generated `.rs` file concatenated in a deterministic
order — `main.rs` first (it is the crate root: it carries the prelude and the
`mod` declarations that name the rest), then the remaining files sorted by name
— with each section after the first introduced by a `// ==== <file>` comment
line. The first section carries no header, so a single-file program's golden is
byte-for-byte its `main.rs`: **the 45 fixtures that never split did not move**,
and the 34 that do grew their module sections (+1939 lines, 0 deleted).

Two details are load-bearing:

- The concatenation lives in ONE place, the test harness, and
  `scripts/regen-example-rust.sh` now runs the harness in rewrite mode
  (`SMELT_UPDATE_EXAMPLE_RUST=1`, narrowed by `SMELT_EXAMPLE_ONLY`). A shell
  script that rebuilt the same sectioned text would be a second implementation
  to drift from the one the suite asserts, and the first sign of the drift
  would be a regeneration that broke `cargo test`.
- The temp project's own directory is redacted to `<example>`. A split module
  carries a `// source: <path>` provenance comment, and under this harness that
  path holds the test process's PID and a nanosecond timestamp — so leaving it
  in would make every golden differ from itself on the next run. That
  placeholder is the only byte in the golden that is not the emitter's.

`end_to_end_rust_goldens_cover_every_generated_file` asserts the shape, so a
harness that quietly went back to comparing `main.rs` alone would be noticed.

## What better measurement uncovered: 20 avoidable-erasure occurrences

`smelt smelt-unknown-report examples` scans the checked-in goldens. It was
therefore reading, for those 34 fixtures, only the runtime prelude. With the
program text visible it reports **20 avoidable-erasure occurrences in 5
fixtures**, where the baseline said 0.

**None of them is new erasure, and none was introduced by this commit.** They
are the corpus's existing generated code, measured for the first time. The
examples baseline is re-snapshotted to 20 in the same commit so the gate keeps
working, and 20 is a FLOOR TO DRIVE DOWN, not a new allowance — the whole
inventory is below so each occurrence can be adjudicated by name.

| n | shape | fixture | source construct |
| ---: | --- | --- | --- |
| 2 | `let failure: SmeltUnknown;` | 76_base64_globals | a `catch (failure)` binding |
| 2 | `let _smelt_tmp_N: SmeltUnknown;` | 76_base64_globals | the same binding's temp |
| 2 | `matches!(error.clone(), SmeltUnknown::Object(value) if value.contains_key("__smelt_domexception"))` | 76_base64_globals | `error instanceof DOMException` |
| 2 | `SmeltUnknown::String("A".into())` | 75_callback_block_return_type | a callback whose return type erased |
| 2 | `SmeltUnknown::String((closure_arg_N.clone()).into())` | 75_callback_block_return_type | the same callback |
| 1 | `let _smelt_tmp_N: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into({ let smelt_callback = Rc::new(\|..\|` | 75_callback_block_return_type | that callback's erased list destination |
| 1 | `pub(crate) async fn requested(input: SmeltUnknown)` | 78_request_input_forms | `input: string \| Request \| URL` |
| 2 | `requested(SmeltUnknown::String(..))` at the call sites | 78_request_input_forms | passing a concrete string into it |
| 1 | `matches!(input.clone(), SmeltUnknown::Object(value) if value.contains_key("__smelt_request"))` | 78_request_input_forms | `input instanceof Request` |
| 1 | `pub(crate) fn tag_of(value: SmeltUnknown) -> String` | 82_data_view_shared_buffer | `function tagOf(value: unknown)` |
| 1 | `pub(crate) fn width_of(value: SmeltUnknown) -> f64` | 82_data_view_shared_buffer | `function widthOf(value: unknown)` |
| 1 | `matches!(value.clone(), SmeltUnknown::Object(value) if value.contains_key("__smelt_dataview"))` | 82_data_view_shared_buffer | `value instanceof DataView` |
| 1 | `SmeltUnknown::String(value) => {` | 80_optional_chain_union_and_throw | a `SmeltUnion8` erased for an indexed read |
| 1 | `SmeltUnknown::Array(values) => {` | 80_optional_chain_union_and_throw | the same match's arm |

### Reading the inventory

Every one of the 20 falls into a shape the `SmeltUnknown enforcement` policy
already names as a genuine boundary — a source `unknown` parameter, a `catch`
binding (which TypeScript types `unknown`), an erased-callback ABI, or a value
inspected through RUNTIME NARROWING. What is missing is line-level EVIDENCE for
the classifier, not a justification:

- The narrowing tests carry it already: `contains_key("__smelt_<marker>")` is
  an `instanceof` probe against a branded host marker and nothing else looks
  like that.
- The match ARMS in fixture 80 are continuation lines of a construct whose head
  line (`slot.clone().into_smelt_unknown()`) is classified legitimate. The
  classifier is already stateful for exactly this reason (`in_prelude_helper`),
  so an occurrence inside a construct whose head is a boundary could inherit
  it.
- A `SmeltUnknown` PARAMETER has no such evidence, and should not get a blanket
  rule: a source `unknown` and a union that merely lost its shape emit the
  identical line, and telling those two apart is the metric's core job. The
  leverage there is to make the EMITTER leave evidence — a provenance comment
  at a signature whose source spelling was `unknown` — which is the same
  "explicit boundary adapter" principle the policy states, applied to the
  report.

### Why none of that is in this commit

A `classify_line` change moves the es-toolkit and remeda numbers too, and this
commit is a test-infrastructure change that must not move a ratchet. So the
classifier is untouched here and the inventory is recorded instead.

**Recommendation for the next round:** one commit that (1) teaches
`classify_line` the branded-marker narrowing shape and boundary-construct
inheritance, with a regression test per shape, (2) adds the emitter provenance
comment for a source-`unknown` signature, and (3) re-snapshots all three
baselines together with the deltas stated. The residue after that is real
avoidable erasure in the corpus, and it should go to zero by fixing the
lowering rather than by reclassification.
