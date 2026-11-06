//! Names the generated crate and the measuring tools agree on.
//!
//! `smelt-unknown-report` classifies generated lines by these spellings, so the
//! emitter and the report read them from here and cannot drift apart.

/// Sentinel comment emitted between the fixed runtime prelude and the generated
/// program body.
///
/// Every emitted crate root contains this line exactly once. It is an inert Rust
/// comment (so it never affects compilation) and the single source of truth for
/// where the shared runtime scaffolding ends. Tooling that measures
/// `SmeltUnknown` usage reads it to attribute occurrences above it to the
/// prelude and occurrences below it to program code. Because it is exported,
/// that tooling references this constant instead of hard-coding the string, so
/// the marker and its consumers can never drift apart.
pub const PRELUDE_END_MARKER: &str = "// @smelt:prelude-end — generated program below";

/// Name prefix of the generated erased-record-to-class conversion helpers,
/// `fn <prefix><n>(smelt_value: SmeltUnknown) -> T`.
///
/// Each helper is the inverse adapter at an erased-to-typed boundary: it
/// takes the erased value and rebuilds (or recovers) the typed class. Its
/// body used to be inlined at every call site, where it was classified as a
/// boundary through its `into_smelt_unknown` call; hoisted, the signature is
/// the one line that newly names `SmeltUnknown`, and the SmeltUnknown report
/// keys its boundary classification on this prefix so the same conversion
/// keeps the classification it had inline.
pub const RECORD_CONVERSION_HELPER_PREFIX: &str = "__smelt_from_unknown_record_";

/// Name prefix of the hoisted record-to-record adapters
/// (`EmitContext::record_adapter_helpers`).
pub const RECORD_ADAPTER_HELPER_PREFIX: &str = "__smelt_adapt_record_";

/// Name of the prelude helper that reads an optional erased value at the
/// erased carrier: `Option<SmeltUnknown>` to `SmeltUnknown`, absence becoming
/// `undefined`.
///
/// It converts between two erased spellings of one JavaScript value and adds
/// no erasure: the value was a `SmeltUnknown` on both sides. The emitter used
/// to assert presence here (`.expect(..)`), which panicked on an absent value;
/// the helper's name is what lets the SmeltUnknown report classify the call
/// site as the boundary it is (same reasoning as `erased_list_alias`).
pub const OPTIONAL_UNKNOWN_OR_UNDEFINED_FN: &str = "smelt_optional_unknown_or_undefined";
