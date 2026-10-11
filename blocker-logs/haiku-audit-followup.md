# Haiku audit follow-up: what was acted on, and what needs a decision

Companion to `blocker-logs/haiku-audit.md` (PR #275). Finding IDs (`F….`, `X-…`)
refer to that report's semantic backlog. Everything in §1 was re-read against
the current code on this branch. Rows in §2 are the audit's confirmed findings
unless a row says it was re-checked here (§2.1 reference counts and F1124 were).

## 1. Acted on

### 1.1 The `main` compile break (commit b12bca59)

Six calls to `list_items_render_as_unknown` lacked the `&RenderScope` argument
#256/#259 added while #250 was open. The four calls inside the scope-threaded
recursion (`value_at_type_in`, `value_at_type_text`) ask the TARGET at the
position's `scope` and the SOURCE at `&self.render_scope()`; the two calls in
the scope-less `erase` / `erase_value` verbs use `&self.render_scope()`. The
audit's overlay (`&self.render_scope()` everywhere) is #250's original
behaviour and loses array identity when a generic class passes an erased array
into an erased generic; both alternatives were planted and caught (unit test
plus the runtime fixture). See the commit message for the derivation.

### 1.2 Dead code removed (commit 039672d0)

F0101, F0112, F0124 (coercion.rs), F0839 (frontend-ts no-op dispatch entry),
F0515 (frontend-py redundant guards), F1218 (transpiler
`collect_file_diagnostics` chain). F0264 was not removed: its claim that
`TypeSubstitution::callee_emission` is dead is false (callers in three files); only the
exhaustive-match arm in `RenderScope::substitution` is unreachable.

### 1.3 Duplicated helpers consolidated (commits 9c28477f, c13462b4, 93e40a0a, 179f37fb, b88f5485)

F0009, F0014, F0018, F0019, F0026, F0028, F0036, F0038, F0087, F0090, F0191,
F0219, F0279 (codegen; the dynamic-shape predicate alone replaced 37 inline
copies), F0644, F0711, F0716, F0736, F0821, F0838, F0841 (frontend-ts
lowering), F0994, F1002, F1020, F1026, F1029 (frontend-ts test helper).
Emitted Rust is unchanged: the e2e examples and snapshot tests pass, and the
es-toolkit crate generated with and without these commits is byte-identical.

That is 31 of the 377 duplicated-helper rows. The remaining rows were not
attempted in this pass; §2.4 lists the ones that have already drifted into
behaviour differences, which are bugs rather than consolidations.

### 1.4 The audit's own comment edits (commits 12c13756, b1d8665d, 3d3311d7, d7e7b814)

A second review read all 361 substantive comment hunks the audit rewrote: 299
accurate, 53 inaccurate (about a third introduced by the audit itself, the rest
kept from text that was already wrong), 9 doubtful. All 53 are corrected, each
re-checked against the code first, plus two stale docs the review surfaced.
Separately, 83 doc comments that `main` had written twice in a row were
collapsed. Comment lines only.

Of the 9 doubtful ones, four were clear enough to settle (commit 58ca0cab: the
self-referencing `operators.rs:1306` note and three `//!` test summaries that
named only part of their file). Five are left as they are because the evidence
did not settle them: #49 (`typed_array.rs:153`, loose rather than false),
frontend-py `pytest.rs:1`, `function_statics.rs:104`, smelt-stdlib
`host_object.rs:494` (ReadableStream `JSON.stringify`), `manifest.rs:399`.

## 2. Needs a decision

### 2.1 Unused public API (not removed: removing public API is your call)

Each was re-counted with `git grep -wP` over `crates/` (count = definition plus
re-exports only).

| ID | Item | Note |
|---|---|---|
| F0556 | `smelt_frontend_py::test_support::is_recognized_pytest_api` (and `is_recognized_pytest_mark`, used only by it) | `pub mod test_support` |
| F0579 | `smelt_frontend_ts::checker::check` (+ `run_tsc`, `run_oxlint`) | named in specs/frontend-py-missing-vs-ts.md as the model for a planned Python checker |
| F1064 | `smelt_hir::type_normalize::is_optional_like` | |
| F1122 | `smelt_mir::Mir::push_closure` | |
| F1133, F1134 | `ResolvedModuleTypes::resolved_return_count` / `resolved_param_count` | their docs claim "used by tests / diagnostics"; they are not |
| F1152, F1153 | `NativeAdapterRegistry::missing` / `versions` | adapter versions never reach the specialization cache key (producers hard-code `Vec::new()`): a latent cache-invalidation gap, not just dead code |
| F1154 | `smelt_specialize::DefinitionKind` | |
| F1165 | `SandboxOutput.elapsed` | written, never read |
| F1166, F1167 | `MacOsSandboxBackend::from_path`, `WindowsAppContainerBackend::from_path` | |
| F1168 | `OciSandboxBackend` (never constructed) | sandbox.rs error messages tell users to configure the OCI fallback that nothing builds |
| F1175–F1177 | `smelt_stdlib::{UnsupportedForm, StdlibDiagnostic, StdlibDiagnostic::unsupported}` | |
| F1191–F1197 | `smelt_stdlib::rules::{SourceLanguage, ApiNamespace, ReceiverKind, ApiShape, ArgShape, ReturnShape, EffectKind}` | a rules vocabulary nothing uses |
| F1204 | `runtime_symbols::strings::DECODE_URI_OCTETS` | doc says codegen calls it; codegen emits the name literally |
| F1206 | `smelt_test::panic_message` | no crate depends on smelt-test |

### 2.2 Dead code that is public or is emitted into generated crates

| ID | Item | Why not removed |
|---|---|---|
| F1124 | `smelt_mir::Rvalue::JsonParse` (never constructed; `JSON.parse` lowers to `BuiltinFn::JsonParse`) | public enum variant |
| F1182, F1183 | `HostExportKind::Type`, `HostExport.kind` (smelt-stdlib) | public |
| F1199 | `RuleId::TsBlobRead`, `TsTypedArrayRead`, `TsResponseRead`, `TsRequestRead` | public enum variants |
| F1137 | `smelt_shared_capture` / `smelt_next_capture_scope` (smelt-runtime, emitted for every capturing closure, never called) | public runtime API, and removal changes every generated crate |
| F1141–F1143 | `SmeltList::{replace_all, get_index, set_index}` (smelt-runtime) | same |
| F0319, F0322, F0323, F0358, F0370 | emitted prelude members nothing calls (`SmeltBody::from_chunks` + `SmeltBodyPayload::Stream`, `SmeltFormDataValue::{to_text, is_file}`, `smelt_missing_{property,index}_value`, four `SmeltMatch` accessors) | changes generated output; some look like placeholders for planned features (streams) |
| F0121 | `if let SmeltUnknown::String(value) = value { value.to_string() } else { value.to_string() }` in emitted list extraction | behaviour-identical (`Display` writes a `String` arm's content) but changes four examples' expected.rs |
| F1213–F1215 | `Project.version/description`, `Config.strict`, `Runtime.clone_strategy` (transpiler config) | manifest schema; specs/cli.md says some values must be rejected and nothing enforces it — decide whether to implement or drop |

### 2.3 Unreachable code that reveals a missing feature (frontend-py)

(Audit-confirmed; not re-verified in this pass.) F0489, F0490, F0491, F0498: dotted alternatives (`dataclasses.dataclass`,
`abc.ABCMeta`, `abc.ABC`, `typing.Generic[T]`, `typing.Protocol`,
`enum.IntEnum`, `abc.abstractmethod`) can never match because
`decorator_simple_name` / `expr_simple_name` / `base_class_name` return only
the last segment or bare names. So `class C(abc.ABC)` is rejected as a complex
base. Deleting the alternatives would bake the gap in; making the helpers
return the dotted path is a feature change. Also `specialization.rs:726` (F0533,
F0534) repeats the same name tests.

Status: resolved on branch `fix/py-dotted-stdlib-names`. All six rows were
re-checked against `main` first (`class C(abc.ABC)` hits "complex base class
expression", `metaclass=abc.ABCMeta` needs specialization, and
`@helpers.abstractmethod` / `@attrs_alias.dataclass` were taken for the stdlib
names). The name helpers were not made to return dotted paths. The frontend
now resolves each decorator, base and metaclass through the module's bindings
(`crates/smelt-frontend-py/src/name_resolution.rs`) and matches the qualified
name.

### 2.4 Duplicates that have already drifted (bugs, not consolidations)

As the audit reports them (verdict "confirmed"); not re-verified in this pass
except F0288 and F0310, which were re-read and hold (by reading the code, not
by running a program). None is fixed here because each changes behaviour.
F0288 is the most contained: `tuple_to_set_text` should pick the container the
way `list_to_set_text` does (`type_is_hash_set_key_safe`).

| ID | Drift |
|---|---|
| F0001 | concrete `from_string_parts` / `from_blob_parts` vs the erased `smelt_blob_parts_bytes` disagree on blob parts |
| F0023 | string-to-number emitted as `parse::<f64>().unwrap_or(NaN)` in two places; JS `Number("")` is 0 and `Number(" 1 ")` is 1 |
| F0044 | `type_mentions_type_params` skips Class args, JsMap and Generator and caps depth at 16; `type_mentions_any` does not |
| F0185 | three `type_contains_function` copies; one recurses into class fields, two answer false for every class |
| F0288 | `tuple_to_set_text` always emits `SmeltPrimSet`; list-to-set and `rust_type` use `SmeltJsSet` for non-hash-safe elements such as `f64` |
| F0310 | `union_member_matches_kind` omits `Type::JsMap` from Object; `static_tag_check` includes it |
| F0611 | callback `.at(i)` skips `coerce_at_index_operand`, which `collection_at_call` runs |
| F0620 | array-literal identifier arm re-resolves names with a reduced copy (no undefined/const/collection handling) |
| F0669 | `DOMException` is missing from six of the seven error-constructor name lists |
| F0684 | `sequence_expression_statement` dropped the logical-assignment branch of `statement_in_block` |
| F0794 | two conditional-join implementations in `new_expr.rs` produce different result types |

### 2.5 Special-case spellings (CLAUDE.md "no special cases")

Not changed: each needs a general rule that is a design decision, and most
change emitted Rust in the library corpora, which only the label-gated
regression jobs exercise. Grouped by a keyword pass over the 142 rows
(approximate):

- **Library- or idiom-keyed (41):** F0190 (remeda `MatchFnResult` as a stdlib
  class), F0602 (callback factories `has`/`omit`/`isNot`/`prop`/`emitEvent`/
  `async.pipe`/`_.negate(_.isNil)`), F0615 (lodash predicate names), F0627
  (`_.forEach`), F0629 (Strapi `async.map`), F0631 (date-fns
  `replace(re, m => m.toUpperCase())`), F0549 (Python `requests.get`), F0505
  (numpy/pandas diagnostics), F0518 (Rich `NullFile`), F0492 (Django models),
  F0488, F0523, F0593, F0622, F0623, F0636, F0638, F0670, F0713, F0720, F0723,
  F0746, F0772, F0811, F0818, F0822, F0859–F0861, F0864, F0890, F0891, F0896,
  F0897, F0905, F0913, F0915, F0916, F1125, F1151, F1174.
- **Decisions driven by emitted Rust text (13):** F0022 (`" as f64"` search
  picks the comparison type), F0045, F0069, F0072, F0078, F0119, F0120,
  F0125, F0130, F0131, F0140, F0177, F0250. The text test is often the only
  precise "does this render as `SmeltUnknown` here" signal; replacing it needs
  a scope-aware MIR predicate per site.
- **Keyed on user identifiers (13):** F0157/F0244/F0336/F0365/F0396 (`main`),
  F0160 (locals named `index…`), F0497/F0501 (`self`/`cls`), F0106, F0495,
  F0662, F0735, F0904.
- **Builtin or host names (75):** mostly per-class arms where a registry
  exists (`instanceof` arms F0051–F0059, `for…in` hidden keys F0347,
  reflection constructors F0386–F0388). Lower priority: JS/Python builtins are
  a fixed vocabulary, but several duplicate a registry the code already has.

### 2.6 `SmeltUnknown`

No erasure was reduced in this pass. Every candidate below changes emitted
types in the library corpora, and the only local check (the examples corpus)
already has zero avoidable erasure, so a reduction could not be shown locally to
both lower the es-toolkit count and keep the generated crates compiling. That
needs the `run-regressions` jobs. §3.1 has the measured state, including a
pre-existing +521 that blocks the es-toolkit ratchet. The 64 audit rows split
into:

- missing boundary comment at a site that IS a dynamic boundary (erased
  receiver, `throw`, host interop): documentation only;
- silent fallbacks that erase on a rendering error instead of failing
  (F0188, F0361, F0364, F0374–F0376, F0379, F0114, F0107): changing them turns
  a generated `SmeltUnknown::Null` into a transpile error, which can break
  corpus builds;
- knowable shapes erased in lowering (F0098, F0205, F0239, F0276, F0285,
  F0303, F0646, F0687, F0701, F0702, F0743, F0751, F0760, F0762, F0781,
  F0782, F0791, F0815, F0827, F0830, F0866, F0906, F0910): each changes
  emitted types in the library corpora and needs the regression jobs.

### 2.7 Long functions and panic paths

No long function was split: most of the 139 rows are the core emitters
(`value_at_type_in`, `rvalue_text_for_dest_inner`, `call_text`, …), which are
under active feature work, and splitting them mid-feature is what the brief
rules out.

Panic paths were not changed. The ones reachable from accepted source and
worth fixing first, with the semantics each needs:

- F0225: Python `xs.insert(i, v)` panics for `i < 0` or `i > len`; Python
  clamps.
- F0226 / F0238: Python `pop()` on an empty list or missing key panics;
  Python raises `IndexError` / `KeyError`, which `except` must be able to catch.
- F0296 / F0297: `int(" 42 ")` panics (Python strips whitespace), and
  `int("abc")` / `float("abc")` panic instead of raising `ValueError`. The
  same casts serve TypeScript `BigInt`/`parseInt`-style paths, so the fix is
  per source function, not one trim.
- F0218: `[1,2,3].with(-10, v)` silently writes index 0; JS throws `RangeError`.
- F0129: `Array.from(5)` panics; JS returns `[]`.

## 3. Measurements

### 3.1 es-toolkit `SmeltUnknown` ratchet: +521 avoidable, all from #250

es-toolkit at the pinned ref (`e008a281`), transpiled with `smelt probe` and
scanned with `smelt smelt-unknown-report --baseline
blocker-logs/smelt-unknown-baseline-es-toolkit.json`:

| build | avoidable | legitimate | prelude | total |
|---|---|---|---|---|
| committed baseline | 25,424 | 35,592 | 2,806 | 63,822 |
| this branch | 25,945 (+521) | 35,071 (−521) | 2,806 | 63,822 |
| this branch, audit overlay instead of b12bca59 | identical generated crate (`diff -r`: 0 files) | | | |
| this branch, #250's two `erase` aliases disabled | 25,492 (+68) | 35,524 | 2,806 | 63,822 |
| this branch, all four #250 aliases disabled | 25,424 (+0) | 35,592 | 2,806 | 63,822 |

So:

- This PR's own changes move es-toolkit's generated Rust by zero bytes. The
  scope choice in b12bca59 only differs from #250's original semantics where a
  narrowed callee scope and the caller scope disagree, which es-toolkit never
  hits.
- The whole +521 is #250's aliasing (`SmeltUnknown::Array(list.into())` in
  `erase`/`erase_value`: 453; `Into::<SmeltList<_>>::into(..)` in
  `value_at_type_in`/`value_at_type_text`: 68). No `SmeltUnknown` was added
  (the total is unchanged); the same occurrences moved from legitimate to
  avoidable. The largest moved shape is `!(SmeltUnknown::Array(a.into()) ==
  SmeltUnknown::Array(b.into()))` (228), whose pre-#250 spelling rebuilt the
  array through the per-element `into_smelt_unknown()` adapter that
  `classify_line` counts as a boundary marker; the smaller shapes were not
  inspected one by one.
- #250 merged into a `main` that did not compile, so the blocking es-toolkit
  ratchet never ran on it. It will fail on the next push to `main` (and on any
  PR labelled `run-regressions`) whether or not this PR merges.

**Decision needed.** `Into::<SmeltList<_>>::into(` cannot become a boundary
marker: it is also the general list choke point (`call_runtime.rs:358`) for
every list-typed rvalue, so it would hide real avoidable erasure. Options:

1. Emit the alias through a named helper (for example a prelude
   `smelt_erased_list_alias`) used only where both sides already render as
   `SmeltList<SmeltUnknown>`, add it to `classify_line` with the CLAUDE.md
   boundary comment and a regression test, and keep the baseline. This is the
   precedent `smelt_typeof(` and `smelt_own_symbol_key_value` set: a respelled
   boundary keeps its classification.
2. Revert #250's aliasing, giving up its 1.98× `uniqueBy` speedup.
3. Treat the 521 as real erasure to remove (they are out-of-scope generic and
   `unknown` lists) and leave the ratchet red until they are.

Re-snapshotting the baseline upward is not an option under CLAUDE.md.

### 3.2 Examples invariant

`smelt smelt-unknown-report examples --baseline
blocker-logs/smelt-unknown-baseline.json --fail-on-regression`: avoidable 0,
exit 0.
