//! Color and styling constants for the Smelt GUI.

use gpui::{Hsla, rgb};

/// Background for the main window.
pub(super) fn bg_primary() -> Hsla {
    rgb(0x1E_1E_2E).into()
}

/// Background for editor panels.
pub(super) fn bg_surface() -> Hsla {
    rgb(0x18_18_25).into()
}

/// Background for the header area.
pub(super) fn bg_header() -> Hsla {
    rgb(0x31_32_44).into()
}

/// Primary accent color (blue).
pub(super) fn accent() -> Hsla {
    rgb(0x89_B4_FA).into()
}

/// TypeScript panel accent color.
pub(super) fn typescript() -> Hsla {
    rgb(0x89_B4_FA).into()
}

/// Python panel accent color.
pub(super) fn python() -> Hsla {
    rgb(0xFA_E3_82).into()
}

/// Rust output panel accent color.
pub(super) fn rust() -> Hsla {
    rgb(0xF9_AF_74).into()
}

/// Success/green accent.
pub(super) fn success() -> Hsla {
    rgb(0xA6_E3_A1).into()
}

/// Error/red accent.
pub(super) fn error() -> Hsla {
    rgb(0xF3_8B_A8).into()
}

/// Primary text color.
pub(super) fn text_primary() -> Hsla {
    rgb(0xCD_D6_F4).into()
}

/// Dimmed/secondary text.
pub(super) fn text_muted() -> Hsla {
    rgb(0xA6_AD_C8).into()
}

/// Border color.
pub(super) fn border() -> Hsla {
    rgb(0x45_47_5A).into()
}
