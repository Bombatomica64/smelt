# Python Frontend Gaps vs TypeScript Frontend

This list tracks behavior currently present in `crates/smelt-frontend-ts` but missing in `crates/smelt-frontend-py`.

## 1) External checker API parity

- [ ] Add a Python-side `check(...)` API equivalent to TS `checker::check(path)`.
- [ ] Define Python checker pipeline/tooling contract (TS currently shells to `oxlint` + `tsc`; Python has no parallel crate-level checker entrypoint yet).

Evidence:
- TS has `crates/smelt-frontend-ts/src/checker.rs` and exports `pub mod checker;`.
- Python crate has no `check` function/module.

## 2) Type-test no-op API parity

- [ ] Add Python no-op lowering for type-test-only assertions similar to TS `expectTypeOf` / `expectType` / `assertType` chain handling.

Evidence:
- TS explicitly recognizes these builtins in `src/test_support.rs` and call lowering.
- Python frontend has pytest assertion lowering, but no typing-test assertion API equivalent.

## 3) Test table API parity (`describe.each` style suites)

- [ ] Add Python equivalent for suite-level table expansion semantics that match TS `describe.each(...)` behavior (not only per-test parametrization).

Evidence:
- TS lowers both `test.each(...)` and `describe.each(...)`.
- Python lowers `pytest.mark.parametrize` for tests, but no suite-level table expansion construct with TS-like nesting behavior.

## 4) Date object method surface parity

- [ ] Add Python lowering equivalents for TS `Date` instance part extraction and mutation family used in TS frontend:
- `getFullYear/getUTCFullYear`, `getMonth/getUTCMonth`, `getDate/getUTCDate`, `getDay/getUTCDay`, `getHours/getUTCHours`, `getMinutes/getUTCMinutes`, `getSeconds/getUTCSeconds`, `getMilliseconds/getUTCMilliseconds`.
- `setFullYear/setUTCFullYear`, `setMonth/setUTCMonth`, `setDate/setUTCDate`, `setHours/setUTCHours`, `setMinutes/setUTCMinutes`, `setSeconds/setUTCSeconds`, `setMilliseconds/setUTCMilliseconds`.

Evidence:
- TS has dedicated `DatePart` lowering paths for these calls.
- Python currently covers `datetime.datetime.now/utcnow/fromtimestamp`, but not this broader object-method matrix.

## 5) Object merge/decoration parity

- [ ] Add Python lowering equivalent for TS `Object.assign(...)` support, including:
- homogeneous dict merge lowering,
- callable decoration shape (`callable + static properties`) parity.

Evidence:
- TS has dedicated `Object.assign` lowering with callable decoration handling.
- No equivalent Python helper path exists for `dict`-merge-as-assign semantics with callable metadata augmentation.

## 6) Shared stdlib rule symmetry gaps

- [ ] Evaluate and add Python-facing equivalents for TS-only rule IDs where language-equivalent surface exists:
- `TsFetch` parity shape beyond `requests.get` (method/body/header variants).
- `TsDateToIsoString` parity shape (Python-side ISO datetime string conversion rule form).

Evidence:
- Rule set includes TS-only entries (`TsFetch`, `TsDateToIsoString`) and Python has narrower, separate mappings.

## 7) API shape parity for lint/type diagnostics output

- [ ] Unify frontend checker return type conventions (TS checker currently returns `Result<(), Box<dyn Error>>`; Python has no checker API).
- [ ] Move both to shared structured diagnostics contract (`Vec<SmeltError>` style) for check-phase output.

Evidence:
- Current TS checker API is unstructured `Box<dyn Error>` and Python has no equivalent API.

## Notes

- This is a parity list only; it does not imply every TS feature must be implemented identically in Python syntax.
- Existing Python strengths not treated as gaps (already present): pytest fixtures/parametrize/raises lowering, set operations, broad `math.*` support.
