# Phase 6 Completion Plan: Registry First, Repo-Green Driven

## Summary

Finish Phase 6 by first introducing a real typed stdlib dispatch registry, then drive implementation through the first external green targets:

1. **date-fns first**: finish the `quartersToMonths` target slice.
2. **Rich second**: implement the Python object/protocol pieces needed by `NullFile`.
3. **Effect and HTTPX next**: use them as stress targets after the simpler first-green slices are stable.
4. **Then broaden stdlib**: implement Date/datetime, URL, Python IO, broader regex, and remaining collection/object gaps.
5. **Defer NumPy/pandas**: write an explicit decision note and targeted diagnostics for Phase 6, but do not implement NumPy/pandas in this phase.

Implementation should be split into vertical slices. Each slice gets focused tests, full `cargo test`, `cargo check`, `cargo clippy`, then its own commit and push.

## Locked Architectural Decisions

- **Phase 6 success definition**: repo-green first, not literal completion of every unchecked long-tail checklist item.
- **Registry timing**: introduce registry before more feature work.
- **Registry strength**: full dispatch for migrated families, not docs-only.
- **Migration strategy**: family-by-family. Build dispatch infrastructure now, migrate first-green-related families immediately, and migrate older mappings only when touched.
- **Backend policy**: use familiar Rust crates first, behind swappable mapping/codegen modules.
- **NumPy/pandas**: defer with a written decision note and clear unsupported diagnostics.
- **Date/URL/IO**: implement a broad practical subset after first-green blockers.
- **Commit strategy**: one commit/push per vertical feature slice.

## New Architecture

### New Crate: `crates/smelt-stdlib`

Add a workspace crate `smelt-stdlib` with no frontend AST dependencies.

It owns shared stdlib mapping metadata:

- `RuleId`
- `SourceLanguage`
- `ApiNamespace`
- `ReceiverKind`
- `ApiShape`
- `ArgShape`
- `ReturnShape`
- `EffectKind`
- `BackendDependency`
- `UnsupportedForm`
- `StdlibDiagnostic`

The crate must not lower AST directly. It defines rule identity, supported shapes, dependency metadata, and diagnostic text.

### Registry Dispatch Shape

Each frontend gets a local dispatcher that uses shared `smelt-stdlib` rules:

- TypeScript: `crates/smelt-frontend-ts/src/lowering/stdlib_dispatch.rs`
- Python: `crates/smelt-frontend-py/src/lowering/stdlib_dispatch.rs`

Each migrated family must follow this flow:

1. Parse source call/member shape into a frontend-local `CallShape`.
2. Query `smelt-stdlib` for a matching `RuleId`.
3. Dispatch on `RuleId` to typed lowering code.
4. If source resembles a known API but unsupported arguments are used, emit `StdlibDiagnostic`.
5. If source is not a known API, return `Ok(None)` so normal lowering continues.

No newly implemented stdlib feature should add more open-coded top-level string matching in `lowering.rs`.

### Codegen Dependency Registry

Move dependency decisions behind shared backend metadata.

Current direct scans for `reqwest`, `serde_json`, `regex`, and `rand` can remain initially, but the plan must introduce:

- `BackendDependency::Reqwest`
- `BackendDependency::SerdeJson`
- `BackendDependency::Regex`
- `BackendDependency::Rand`
- `BackendDependency::Chrono`
- `BackendDependency::Url`

Codegen can still scan MIR rvalues, but generated dependency names/versions/features must be centralized in one module.

Add:

- `crates/smelt-codegen-rust/src/deps.rs`
- `crates/smelt-codegen-rust/src/stdlib.rs`

Keep source emission helpers outside `lib.rs` per `AGENTS.md`.

## Slice 0: Stabilize Current Work

Before new architecture work:

1. Run:
   ```bash
   cargo test
   cargo check
   cargo clippy
   ```
2. Commit current completed Phase 6/Test-TODO progress if still uncommitted.
3. Push.
4. Do not revert unrelated dirty work.

Commit message:
```text
Checkpoint phase 6 repo-target progress
```

## Slice 1: Typed Registry Foundation

### Files

Add:

- `crates/smelt-stdlib/Cargo.toml`
- `crates/smelt-stdlib/src/lib.rs`
- `crates/smelt-stdlib/src/rules.rs`
- `crates/smelt-stdlib/src/diagnostics.rs`
- `crates/smelt-stdlib/src/deps.rs`

Update workspace `Cargo.toml`.

Add frontend dispatch modules:

- `crates/smelt-frontend-ts/src/lowering/stdlib_dispatch.rs`
- `crates/smelt-frontend-py/src/lowering/stdlib_dispatch.rs`

Add codegen modules:

- `crates/smelt-codegen-rust/src/deps.rs`
- `crates/smelt-codegen-rust/src/stdlib.rs`

### Initial Migrated Families

Migrate only these families first:

- TS/Python JSON
- TS/Python regex match basics
- TS/Python random basics
- TS fetch / Python requests.get
- TS exported constant folding diagnostics where applicable

### Acceptance

- Existing behavior unchanged.
- Existing tests still pass.
- New tests prove unsupported known API forms get a dedicated stdlib diagnostic.
- `cargo test`, `cargo check`, `cargo clippy` pass.

## Slice 2: date-fns First Green

Goal: make the `date-fns` `quartersToMonths` slice compile and run generated Rust tests.

### Implement

- Finish exported constant expression support needed by `src/constants/index.ts`.
- Support remaining foldable primitive expression shapes found in the slice:
  - unary `+`
  - parenthesized expressions
  - nested identifiers
  - arithmetic over foldable numeric constants
  - selected pure `Math.*` calls already supported by runtime lowering
- Ensure imported constants from `index.ts` modules remain available.

### Tests

Add CLI fixture mirroring:

- `src/constants/index.ts`
- `src/quartersToMonths/index.ts`
- `src/quartersToMonths/test.ts`

Acceptance:

- Generated crate runs `cargo test`.
- All four `quartersToMonths` test cases pass.

## Slice 3: Python Package Import and Constructed Module Constants

Goal: unblock Rich `NULL_FILE = NullFile()` and real package imports.

### Implement

- Module-level constructed constants:
  - `NAME = ClassName(...)`
  - only for known class constructors
  - store as importable const/module binding metadata
- `from rich._null_file import NULL_FILE, NullFile` across manifest/package paths.
- Package-level export aliases in `__init__.py`.
- Keep `__all__` as metadata only.

### Tests

- Frontend test for `NULL_FILE = NullFile()`.
- CLI test for importing both `NULL_FILE` and `NullFile` from a package path.
- Regression for `__all__` not emitting runtime statements.

## Slice 4: Python Protocol/Dunder Support for Rich NullFile

Goal: make Rich `NullFile` first green.

### Implement

Supported object/protocol subset:

- `__enter__` and `__exit__` methods.
- `with value as name:` context manager lowering.
- `__iter__` and `__next__` enough for direct iteration tests.
- `__str__` where called by `str(value)` or display/assert behavior.
- Methods returning `self`.
- No broad dynamic protocol dispatch; only statically known class methods.

HIR/MIR additions as needed:

- `ContextEnter`
- `ContextExit`
- protocol method call representation if existing method call cannot express it cleanly.

### Tests

- Unit tests for context manager lowering.
- Unit tests for iterator protocol lowering.
- Unit tests for `str(obj)` using `__str__`.
- CLI Rich-like `NullFile` fixture that emits Rust tests and passes.

## Slice 5: HTTPX Enum/Classmethod Support

Goal: unblock `httpx/_status_codes.py` and simple status-code tests.

### Implement

- Targeted `IntEnum` lowering:
  - enum members as integer constants
  - enum class as class/type metadata
  - member lookup `codes.OK`
- Class body self-reference:
  - `codes.__new__`
  - references to class name inside class body
- `@classmethod`
- `cls` parameter binding
- class-level method calls:
  - `codes.get_reason_phrase(...)`
  - `httpx.codes.get_reason_phrase(...)`

Do not implement full Python metaclass behavior.

### Tests

- `IntEnum` subclass fixture.
- Classmethod fixture.
- Class self-reference fixture.
- HTTPX-like status code fixture.

## Slice 6: Effect First Useful Slice

Goal: make the current Effect typeclass numeric slice pass enough to generate and run simple tests.

### Implement

- Remaining exported non-primitive value expression forms needed by the slice.
- Targeted handling for `dual(...)` if it can be represented as a simple wrapper around existing functions.
- `readonly unknown[]` and unknown boundary behavior needed by the slice.
- Array iteration and readonly array parameter handling.

### Constraints

- Reject `any`.
- Keep `unknown` distinct.
- Operations on `unknown` require narrowing or explicit supported shape.
- Do not overbuild a general Effect runtime.

## Slice 7: Broad Dependency-Backed Stdlib

After date-fns and Rich are green, implement broader requested Phase 6 APIs.

### Date/datetime with `chrono`

TypeScript:

- `Date.now()`
- `new Date(timestamp)`
- `toISOString()`
- basic getters needed by fixtures

Python:

- `datetime.datetime.now()`
- `utcnow()`
- `fromtimestamp()`
- `isoformat()`
- `date`
- `timedelta` basic construction/arithmetic

### URL with `url`

TypeScript:

- `new URL(text)`
- `.href`
- `.protocol`
- `.host`
- `.hostname`
- `.pathname`
- `.search`

Python:

- Defer broad urllib unless needed.
- Add targeted diagnostics.

### Python IO

Implement std-backed file subset:

- `open(path, "r")`
- `open(path, "w")`
- text `read()`
- text `write()`
- context manager lowering for files

Binary mode may be added if straightforward; otherwise targeted diagnostic.

### Regex Expansion

TypeScript:

- regex-backed `String.replace`
- `replaceAll`
- `match` only for simple boolean/list cases if statically representable

Python:

- `re.sub`
- `re.split`
- `re.compile` as a compiled regex value if needed

Flags/captures must either lower correctly or produce targeted diagnostics.

## Slice 8: Remaining Practical Collection/Object Gaps

Implement or targeted-reject:

TypeScript:

- `Object.fromEntries`
- `Object.assign`
- `Array.sort` without comparator
- `Array.sort` with comparator only after callback capture support
- `Array.splice` targeted diagnostic unless mutation semantics are fully modeled
- `delete obj[key]` targeted diagnostic unless implemented

Python:

- list/dict/set comprehensions if needed by target repos
- `isinstance` and `issubclass` where statically decidable
- `str.index` / `rindex` only if exception semantics are represented
- `sorted(key=...)` only after callback values/captures are supported

## Slice 9: NumPy/Pandas Decision Note

Create:

- `specs/native-data-libraries.md`

Content:

- NumPy is deferred from Phase 6 implementation.
- Pandas is explicitly out of scope for v1 Phase 6.
- Future NumPy options:
  - Rust-native `ndarray`
  - Python/native ABI bridge
  - hybrid backend
- Required future decisions:
  - dtype model
  - ownership
  - shape/broadcasting
  - error semantics
  - serialization/FFI boundaries

Add targeted diagnostics for imported NumPy/pandas APIs if encountered.

## Slice 10: Documentation and Exit Criteria Cleanup

### Add/Update Docs

- `specs/stdlib-mapping.md`
- `specs/test-framework-repo-survey.md`
- `IMPLEMENTATION_CHECKLIST.md`
- `Test-TODO.md`

### `specs/stdlib-mapping.md` Format

Each row must include:

- source API
- language
- supported argument shapes
- unsupported argument shapes
- HIR expression
- MIR rvalue
- Rust output
- backend dependency
- known semantic differences
- tests covering it

### Exit Criteria

Phase 6 can close when:

- date-fns first-green target passes.
- Rich first-green target passes.
- Effect numeric slice has a documented pass/fail status with first unsupported construct recorded.
- HTTPX status-code slice has a documented pass/fail status with first unsupported construct recorded.
- Registry dispatch exists and is used for all touched families.
- NumPy/pandas decision note exists.
- Date/datetime, URL, Python IO, and regex expansion have either implemented broad practical subsets or targeted diagnostics.
- `cargo test`, `cargo check`, and `cargo clippy` pass.
- Work is committed and pushed per vertical slice.

## Subagent Implementation Plan

Use subagents only after planning mode ends and implementation starts.

### Main Agent

Owns architecture integration:

- workspace crate setup
- `smelt-stdlib`
- HIR/MIR additions
- codegen dependency plumbing
- final integration
- full verification
- commits/pushes

### Worker 1: TypeScript Registry and date-fns

Write scope:

- `crates/smelt-frontend-ts/src/lowering/stdlib_dispatch.rs`
- targeted TS lowering/tests
- CLI fixtures for date-fns

Must not edit Python frontend.

### Worker 2: Python Registry and Rich

Write scope:

- `crates/smelt-frontend-py/src/lowering/stdlib_dispatch.rs`
- Python import/package/protocol lowering/tests
- Rich-like CLI fixture

Must not edit TypeScript frontend.

### Worker 3: HTTPX Object Model

Write scope:

- Python class/enum/classmethod lowering tests
- object/protocol implementation files assigned by main agent
- HTTPX-like CLI fixture

Must coordinate with Worker 2 to avoid overlapping files.

### Worker 4: Docs and Survey

Write scope:

- `IMPLEMENTATION_CHECKLIST.md`
- `Test-TODO.md`
- `specs/stdlib-mapping.md`
- `specs/native-data-libraries.md`
- `specs/test-framework-repo-survey.md`

Must not touch source code.

### Worker 5: Codegen Modules

Write scope:

- `crates/smelt-codegen-rust/src/deps.rs`
- `crates/smelt-codegen-rust/src/stdlib.rs`
- focused changes in `src/lib.rs` only to route through new helpers
- codegen tests

## Verification Per Slice

Every slice must run:

```bash
cargo test
cargo check
cargo clippy
```

For external-target slices, also run the target fixture command or CLI regression added for that slice.

## Explicit Defaults

- Direct Rust code is preferred over runtime helpers.
- Rust crates are preferred over custom compatibility runtimes.
- Dependency-backed mappings must be isolated behind modules so backend crates can be swapped later.
- Dynamic Python/JS behavior is not guessed. Unsupported forms get source-located diagnostics.
- `any` stays rejected.
- `unknown` is represented distinctly and requires narrowing before use.
- Map/Record internal interchangeability remains acceptable when source typechecking would reject invalid use before Smelt.
- No broad codebase division refactor until each active feature slice is stable.
