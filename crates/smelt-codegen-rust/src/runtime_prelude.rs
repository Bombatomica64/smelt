//! Selection of runtime prelude items from the `smelt-runtime` crate.
//!
//! The runtime helpers that generated crates carry are written as real,
//! compiled, unit-tested Rust in `smelt-runtime` (see that crate's docs). This
//! module is the emitter's half of that arrangement: it maps each MIR-computed
//! `needs_*` gate to the runtime items that gate requires, and copies their
//! source text into the generated crate root verbatim.
//!
//! # What this module is not
//!
//! It is not a new gating mechanism. The `needs_*` predicates in `lib.rs` and
//! `stdlib.rs` still decide *whether* a gate is on, with unchanged semantics;
//! [`PreludeGate`] only names the item list each one pulls in. Nor does it make
//! generated crates depend on `smelt-runtime`: the text is inlined, so generated
//! crates stay self-contained and cannot skew against a runtime version.
//!
//! # Failing loudly
//!
//! A gate that names an item the runtime does not define is a build-breaking
//! mistake, not a silent omission: [`emit_gate`] returns an [`EmitError`] naming
//! the gate and the missing item, and the `runtime_manifest_items_resolve` test
//! in `src/tests/runtime_manifest_tests.rs` walks the whole manifest — every
//! gate, not just the ones a given program turns on — so a typo fails the test
//! suite rather than a user's build.

#![expect(
    clippy::redundant_pub_crate,
    reason = "crate-visible helpers are shared with the parent module and the emitter shards"
)]

use crate::{EmitError, rust::CodeWriter};

/// A runtime prelude gate: one `needs_*` predicate's worth of runtime items.
///
/// Each variant corresponds to an existing `needs_*` predicate computed from the
/// MIR. Variants are listed in emission order so the manifest reads like the
/// prelude it produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreludeGate {
    /// `Date.now()` override slot (`needs_date_now`).
    DateNow,
    /// Timezone-offset override slot (`needs_date_timezone_offset`).
    DateTimezoneOffset,
    /// Virtual + wall clock shared by timers and `Date.now()`
    /// (`needs_timer_helpers || needs_date_now`).
    VirtualClock,
    /// Shared mutable closure captures (`needs_shared_captures`).
    SharedCaptures,
    /// `encodeURI` (`stdlib::needs_uri_encode_runtime`).
    UriEncode,
    /// The `smelt_locale_compare` string-collation helper backing
    /// `String.prototype.localeCompare`.
    LocaleCompare,
    /// JS reference-identity minting on its own, for a regex/match program with
    /// no list (`needs_regex && !needs_smelt_list`).
    ObjectIdentity,
    /// The identity-bearing typed list, which also mints ids
    /// (`needs_smelt_list`).
    SmeltList,
}

impl PreludeGate {
    /// Every gate, for exhaustive manifest checks.
    ///
    /// Only the manifest tests read this: emission reaches a gate through its
    /// `needs_*` predicate, never by iterating the list.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "the manifest test enumerates every gate; the emitter selects gates individually"
        )
    )]
    pub(crate) const ALL: &'static [Self] = &[
        Self::DateNow,
        Self::DateTimezoneOffset,
        Self::VirtualClock,
        Self::SharedCaptures,
        Self::UriEncode,
        Self::LocaleCompare,
        Self::ObjectIdentity,
        Self::SmeltList,
    ];

    /// The runtime items this gate emits, in emission order.
    ///
    /// Two gates may name the same item (`SmeltList` re-lists the identity items
    /// because `SmeltList::new` mints an id); the gates are mutually exclusive at
    /// their call sites, exactly as the hand-written blocks were.
    pub(crate) const fn items(self) -> &'static [&'static str] {
        match self {
            Self::DateNow => &["SMELT_DATE_NOW"],
            Self::DateTimezoneOffset => &["SMELT_DATE_TIMEZONE_OFFSET"],
            Self::VirtualClock => &["SMELT_VIRTUAL_MS", "smelt_mono_ms", "smelt_virtual_advance_to"],
            Self::SharedCaptures => &[
                "SMELT_NEXT_CAPTURE_SCOPE",
                "smelt_next_capture_scope",
                "smelt_shared_capture",
            ],
            Self::UriEncode => &["smelt_encode_uri"],
            Self::LocaleCompare => &["smelt_locale_compare"],
            Self::ObjectIdentity => &["SMELT_NEXT_OBJECT_ID", "smelt_next_object_id"],
            Self::SmeltList => &["SMELT_NEXT_OBJECT_ID", "smelt_next_object_id", "SmeltList"],
        }
    }
}

/// Emits every runtime item a gate requires, each followed by a blank line.
///
/// The text is copied verbatim from the runtime crate's source, one line at a
/// time through the writer so indentation and the trailing blank line match the
/// hand-written blocks this replaced byte for byte.
///
/// # Errors
///
/// Returns an [`EmitError`] naming the gate and the item when the runtime does
/// not define an item the gate lists, so a manifest typo can never degrade into
/// a generated crate that is missing a helper.
pub(crate) fn emit_gate(writer: &mut CodeWriter, gate: PreludeGate) -> Result<(), EmitError> {
    for item in gate.items() {
        let source = smelt_runtime::source::item_source(item).ok_or_else(|| {
            EmitError::new(format!(
                "runtime prelude gate {gate:?} requires item `{item}`, which smelt-runtime does not define"
            ))
        })?;
        for line in source.lines() {
            writer.line(line);
        }
        writer.blank_line();
    }
    Ok(())
}
