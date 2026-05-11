# Native Data Libraries

Phase 6 does not implement NumPy or pandas. This note records the decision so data-library imports can fail with targeted diagnostics instead of being mistaken for generic unresolved modules.

## Phase 6 Decision

- NumPy is deferred from Phase 6 implementation.
- pandas is explicitly out of scope for v1 Phase 6.
- Smelt should emit targeted unsupported-stdlib diagnostics when it recognizes imported NumPy or pandas APIs during Phase 6.
- No generic CPython extension fallback is planned for Phase 6.

## NumPy Options For A Later Phase

| Option | Shape | Benefits | Risks |
| --- | --- | --- | --- |
| Rust-native `ndarray` | Lower NumPy arrays into Rust array/tensor values backed by `ndarray`. | Keeps generated Rust native, testable, and independent of a Python runtime. | Requires an explicit dtype, broadcasting, view/copy, and error model before broad coverage is correct. |
| Python/native ABI bridge | Call NumPy through a stable native/Python boundary. | Can preserve more NumPy behavior where Rust-native parity would be expensive. | Ownership, lifetime, interpreter initialization, error propagation, packaging, and cross-platform linking become backend concerns. |
| Hybrid backend | Use Rust-native lowering for simple array operations and a native bridge for selected high-parity operations. | Allows incremental support while reserving escape hatches for hard APIs. | Requires clear serialization/FFI boundaries so generated code stays predictable. |

## Required Future Decisions

- Dtype model: scalar kinds, width, signedness, float behavior, object dtype policy, and promotion rules.
- Ownership: array allocation, views versus copies, mutability, aliasing, and borrowed slice lifetimes.
- Shape and broadcasting: static versus runtime shapes, dimension metadata, broadcasting errors, and reshape semantics.
- Indexing and slicing: scalar indexing, slicing views, advanced indexing, boolean masks, and negative index behavior.
- Error semantics: panic, `Result`, source-language exception emulation, and diagnostic boundaries for unsupported dynamic behavior.
- Serialization and FFI boundaries: when values cross Rust/native/Python boundaries, how errors cross back, and which representations are stable.
- Dependency policy: whether `ndarray`, NumPy C APIs, PyO3, or another bridge owns each supported operation family.

## Phase 6 Diagnostics

Recognized NumPy and pandas imports should report the library name, the unsupported API if known, and the Phase 6 decision:

- `numpy` / `np`: unsupported in Phase 6; NumPy needs a later dtype/shape/ownership design.
- `pandas` / `pd`: out of scope for v1 Phase 6; no dataframe model exists yet.

The diagnostic should be source-located and should not suggest that arbitrary CPython extension calls are supported.
