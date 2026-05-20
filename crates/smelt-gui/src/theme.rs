//! Color and styling constants for the Smelt GUI.
//!
//! Uses a Catppuccin Mocha–inspired palette with language-specific accents
//! and surface elevation levels for visual depth.

use gpui::{Hsla, rgb};

// ── Background layers (darkest → lightest) ──────────────────────────

/// Deepest background for the main window.
pub(super) fn bg_primary() -> Hsla {
    rgb(0x1E_1E_2E).into()
}

/// Editor panel backgrounds — slightly darker for inset feel.
pub(super) fn bg_surface() -> Hsla {
    rgb(0x15_15_22).into()
}

/// Header and panel label backgrounds.
pub(super) fn bg_header() -> Hsla {
    rgb(0x28_29_3D).into()
}

/// Status bar background.
pub(super) fn bg_status_bar() -> Hsla {
    rgb(0x24_25_37).into()
}

/// Hover background for interactive elements.
pub(super) fn bg_hover() -> Hsla {
    rgb(0x3B_3D_54).into()
}

/// Subtle elevated surface for buttons.
pub(super) fn bg_button() -> Hsla {
    rgb(0x35_36_4A).into()
}

// ── Language accents ────────────────────────────────────────────────

/// TypeScript panel accent (blue).
pub(super) fn typescript() -> Hsla {
    rgb(0x89_B4_FA).into()
}

/// Python panel accent (yellow).
pub(super) fn python() -> Hsla {
    rgb(0xFA_E3_82).into()
}

/// Rust output panel accent (orange).
pub(super) fn rust() -> Hsla {
    rgb(0xF9_AF_74).into()
}

// ── Semantic colors ────────────────────────────────────────────────

/// Primary accent (blue).
pub(super) fn accent() -> Hsla {
    rgb(0x89_B4_FA).into()
}

/// Success / green.
pub(super) fn success() -> Hsla {
    rgb(0xA6_E3_A1).into()
}

/// Error / red.
pub(super) fn error() -> Hsla {
    rgb(0xF3_8B_A8).into()
}

/// Dimmed success for subtle backgrounds.
pub(super) fn success_dim() -> Hsla {
    rgb(0x1A_2E_1F).into()
}

/// Dimmed error for subtle backgrounds.
pub(super) fn error_dim() -> Hsla {
    rgb(0x2E_1A_1F).into()
}

// ── Text ───────────────────────────────────────────────────────────

/// Primary text color.
pub(super) fn text_primary() -> Hsla {
    rgb(0xCD_D6_F4).into()
}

/// Dimmed / secondary text.
pub(super) fn text_muted() -> Hsla {
    rgb(0xA6_AD_C8).into()
}

/// Very dimmed text for decorative labels.
pub(super) fn text_faint() -> Hsla {
    rgb(0x6C_70_86).into()
}

// ── Borders ────────────────────────────────────────────────────────

/// Default border color.
pub(super) fn border() -> Hsla {
    rgb(0x45_47_5A).into()
}

/// Subtle border for inset panels.
pub(super) fn border_subtle() -> Hsla {
    rgb(0x31_32_44).into()
}
