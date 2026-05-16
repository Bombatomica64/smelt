# Testing Strategy

This document defines how smelt proves that transpiled Rust behaves like the input TypeScript or Python. Snapshot tests are still useful, but they are not enough: the main correctness signal should come from running source-language tests against generated Rust.

## Goal

The long-term goal is to make TypeScript and Python tests native to the generated Rust crate without rewriting the user's test suite by hand.

For a supported project, this should eventually work as:

```
source code (.ts / .py)
source tests (.test.ts / test_*.py)
        │
        ▼
smelt test
        │
        ├── compile app/library modules to Rust
        ├── compile supported test modules to Rust tests
        ├── lower test framework APIs to smelt's Rust test runtime
        ▼
cargo test
```

The point is not to imitate Node or CPython at runtime. The point is to make tests written for Vitest/Jest-style TypeScript or pytest-style Python become Rust tests that assert against the same transpiled program.

## Why This Matters

Running generated binaries and comparing stdout catches simple programs, but real projects encode their behavior in tests. If smelt can compile both the source and its tests, existing projects bring their own behavioral oracle.

This also tests smelt's core promise directly: typed TS/Python should be portable into Rust without the developer rewriting the program in Rust.

## Test Framework Portability Layer

smelt should provide a small Rust test runtime for source-language testing APIs. This runtime lives below the frontends and above plain `cargo test`.

Framework priority:

- TypeScript v1 should target the Vitest/Jest-style unit-test API first. Vitest is the better default for modern strict TS projects, and Jest compatibility matters because many projects still use Jest-style matchers.
- TypeScript browser/component tooling such as Playwright and Testing Library should be treated as later integration targets. They matter, but their useful boundary is usually browser or DOM behavior rather than pure transpiled library behavior.
- Python v1 should target pytest first. Plain `unittest` can mostly lower later as classes plus assertion methods, but pytest's function-based tests, bare `assert`, `raises`, parametrization, and fixtures are the highest-value path.
- Python property-based testing through Hypothesis is valuable after deterministic unit tests work. Its public API should initially be a generator of Rust test cases or proptest-like cases, not a dynamic Hypothesis runtime clone.

Initial TypeScript targets:

- `describe(name, fn)`
- `it(name, fn)` / `test(name, fn)`
- `expect(value).toBe(expected)`
- `expect(value).toEqual(expected)`
- `expect(value).toStrictEqual(expected)` where supported values have structural equality
- `expect(value).toThrow(...)` once exception lowering exists
- `beforeEach` / `afterEach` after basic function test cases work

Initial Python targets:

- bare `assert expr`
- pytest-style test discovery for `test_*` functions
- `pytest.raises(...)` once exception lowering exists
- simple fixtures after plain function tests work
- parametrization only after the test harness can generate multiple Rust `#[test]` cases from one source test

These APIs should not be implemented by embedding Vitest or pytest. They should lower into Rust test functions and helper assertions. When a test framework feature depends on dynamic runtime behavior that smelt does not support, smelt should reject it with a source-located error.

## Lowering Shape

Each source test becomes a generated Rust `#[test]` or `#[tokio::test]` function.

TypeScript:

```ts
import { add } from "./math";

test("adds numbers", () => {
  expect(add(2, 3)).toBe(5);
});
```

Conceptual Rust output:

```rust
#[test]
fn adds_numbers() {
    smelt_test::assert_same(add(2.0, 3.0), 5.0);
}
```

Python:

```py
from math import add

def test_adds_numbers() -> None:
    assert add(2, 3) == 5
```

Conceptual Rust output:

```rust
#[test]
fn test_adds_numbers() {
    assert_eq!(add(2, 3), 5);
}
```

The generated Rust does not need to preserve the test framework's internal object model. It only needs to preserve test intent for the supported subset.

## Bootstrapping Phases

### Phase 1: Differential Examples

Keep the current end-to-end fixture style, but add a behavioral oracle:

1. Run the original TypeScript or Python program.
2. Build with smelt.
3. Run the generated Rust crate.
4. Compare stdout, stderr, and exit code.

This is still script-oriented, but it catches obvious semantic mismatches before test framework support exists.

### Phase 2: Native Rust Test Emission

Add a minimal test lowering path:

- Detect test files from config or conventional names.
- Lower plain Python `test_*` functions and TS `test(...)` calls.
- Emit Rust `#[test]` functions into the generated crate.
- Run them through `cargo test`.

This phase only supports direct assertions and simple equality.

### Phase 3: Test Runtime Helpers

Introduce `smelt-test`, a Rust crate containing assertion helpers and framework compatibility glue. Keep helpers small and explicit. Do not build a full dynamic test framework unless a supported source feature truly needs it.

`smelt-test` targets public API behavior first. In a perfect end state, smelt may transpile the internals of source-language testing libraries too, but v1 should map their stable public surface into native Rust tests. This keeps the correctness machine practical while still testing user code through the APIs users actually write.

Likely helpers:

- equality and structural equality assertion formatting
- approximate float assertions if a source framework feature requires it
- panic/error matching for lowered exceptions
- async test wrappers
- source span metadata for readable failure messages

### Phase 4: External Project Conformance

Pick small, strict TS/Python libraries with mostly pure functions and simple tests. Compile both their source and supported tests to Rust, then run `cargo test`.

Good first targets have:

- strict TypeScript or fully typed Python
- few dependencies
- deterministic pure functions
- simple imports
- tests written in a small pytest/Vitest/Jest subset

Bad first targets have:

- decorators, reflection, proxies, monkey-patching, `eval`, or metaclasses
- heavy mocking
- untyped test helpers
- filesystem/network effects unless the project is intentionally testing those mappings

### Phase 5: App-Level Black-Box Tests

For Express and FastAPI demos, prefer black-box tests that can run against both original and generated servers. HTTP tests avoid needing full framework test compatibility and prove the generated app behaves correctly at the boundary users care about.

## Relationship to Upstream Test Suites

Upstream language suites are useful as input sources, not as suites to run wholesale.

- TypeScript's test cases are useful for parser, checker, and rejection fixtures.
- Test262 is useful for JS expression and control-flow semantics, filtered down to smelt's strict subset.
- CPython's tests are useful to mine small semantic examples, but Python documents its `test` package as internal to CPython.
- MicroPython's output-comparison tests are a useful model for small Python behavior cases.

All imported cases should go through an allowlist. smelt intentionally rejects large parts of JS and Python, so denylisting unsupported tests from massive suites is the wrong shape.

## Rejection Is Correctness

When a source test relies on unsupported semantics, the correct result is a clear smelt error, not a best-effort Rust test.

Examples:

- TypeScript `any` or runtime type abuse
- JS property access patterns that require prototype semantics
- Python monkey-patching or dynamic attribute lookup
- pytest fixtures that depend on dynamic injection smelt cannot model yet
- Vitest mocking APIs that replace module bindings at runtime

Unsupported test framework features should become negative tests for smelt itself.

## Acceptance Criteria

The testing strategy is working when:

- a source-language test can compile into a Rust `#[test]`
- assertion failures point back to the source test location where practical
- generated tests run under plain `cargo test`
- unsupported test framework features fail before codegen with clear errors
- at least one small TS project and one small Python project can run meaningful original tests as generated Rust tests
