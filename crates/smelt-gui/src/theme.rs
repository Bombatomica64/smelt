//! Color and styling tokens for the Smelt showcase GUI.
//!
//! A bold, product-oriented palette: near-black graphite surfaces with a
//! molten rust/copper primary and language-specific accents (TypeScript blue,
//! Python amber, Rust ember). Sans-serif is used for product/header chrome and
//! monospace only for source, generated Rust, diagnostics, and counters.
//!
//! Every token is exposed as a function returning an [`Hsla`] so call sites read
//! like `theme::primary()` and the palette stays trivially greppable.

use gpui::{Hsla, rgb};

// ── Surfaces (darkest → lightest) ───────────────────────────────────

/// Deepest application background (graphite).
pub(super) fn bg_base() -> Hsla {
    rgb(0x0E_0E_12).into()
}

/// Standard panel background sitting above the base.
pub(super) fn bg_panel() -> Hsla {
    rgb(0x15_15_1B).into()
}

/// Inset surface for editors and generated output.
pub(super) fn bg_surface() -> Hsla {
    rgb(0x1B_1B_23).into()
}

/// Elevated surface for headers, cards, and the example rail.
pub(super) fn bg_elevated() -> Hsla {
    rgb(0x22_22_2D).into()
}

/// Hover background for interactive rows and buttons.
pub(super) fn bg_hover() -> Hsla {
    rgb(0x2C_2C_3A).into()
}

/// Selected/active row background.
pub(super) fn bg_selected() -> Hsla {
    rgb(0x33_24_22).into()
}

// ── Brand + language accents ────────────────────────────────────────

/// Molten rust/copper — the primary brand accent and primary action color.
pub(super) fn primary() -> Hsla {
    rgb(0xE8_64_3A).into()
}

/// Dimmed primary for subtle fills.
pub(super) fn primary_dim() -> Hsla {
    rgb(0x33_20_18).into()
}

/// TypeScript accent (saturated blue).
pub(super) fn typescript() -> Hsla {
    rgb(0x4C_8D_F6).into()
}

/// Python accent (warm amber).
pub(super) fn python() -> Hsla {
    rgb(0xF5_A6_23).into()
}

/// Rust / generated-output accent (ember orange).
pub(super) fn ember() -> Hsla {
    rgb(0xFF_7A_3D).into()
}

// ── Semantic colors ─────────────────────────────────────────────────

/// Success / healthy compile (vivid green).
pub(super) fn success() -> Hsla {
    rgb(0x3F_B9_50).into()
}

/// Dimmed success background.
pub(super) fn success_dim() -> Hsla {
    rgb(0x10_2A_14).into()
}

/// Error / failed compile (rose red).
pub(super) fn error() -> Hsla {
    rgb(0xF2_54_6B).into()
}

/// Dimmed error background.
pub(super) fn error_dim() -> Hsla {
    rgb(0x2E_13_1A).into()
}

/// Neutral/skipped state (muted slate).
pub(super) fn neutral() -> Hsla {
    rgb(0x6E_6A_63).into()
}

// ── Text ────────────────────────────────────────────────────────────

/// Primary high-contrast text (warm white).
pub(super) fn text_primary() -> Hsla {
    rgb(0xF2_ED_E6).into()
}

/// Secondary / muted text.
pub(super) fn text_muted() -> Hsla {
    rgb(0xA8_A2_9A).into()
}

/// Very dimmed text for decorative labels and counters.
pub(super) fn text_faint() -> Hsla {
    rgb(0x6E_6A_63).into()
}

/// Text drawn on top of the primary (copper) fill.
pub(super) fn text_on_primary() -> Hsla {
    rgb(0x12_0A_06).into()
}

// ── Borders ─────────────────────────────────────────────────────────

/// Default border color.
pub(super) fn border() -> Hsla {
    rgb(0x30_30_3D).into()
}

/// Subtle border for inset separators.
pub(super) fn border_subtle() -> Hsla {
    rgb(0x23_23_2D).into()
}
